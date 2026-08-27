/**
 * The IPC contract between the WebView and the Rust core.
 *
 * ## The rule this file exists to enforce
 *
 * **Secrets do not cross into JavaScript.**
 *
 * JS strings are immutable, the engine may copy them during GC, and there is no
 * `zeroize`. Once a password reaches the WebView we permanently lose the
 * ability to say when it stops existing in memory. So the vault lives in Rust,
 * and the WebView is a viewer that knows structure but not content.
 *
 * Concretely:
 *   - `listEntries` and `getEntry` return metadata only. A protected field is
 *     reported as `{ key, protected: true }` with **no value**.
 *   - `copyField` never returns the secret. Rust reads it and writes it to the
 *     clipboard itself.
 *   - `revealField` is the single deliberate exception. See its doc comment.
 */

export type Uuid = string;

/** Milliseconds since the Unix epoch. */
export type Millis = number;

export interface VaultInfo {
	name: string;
	path: string;
	entryCount: number;
	/** KDBX 3.1 / 4.0 / 4.1 — shown in the UI because it affects sync behaviour. */
	format: string;
	/** False for KDBX 3.1, which we can read but not write. */
	writable: boolean;
}

export interface GroupNode {
	uuid: Uuid;
	name: string;
	entryCount: number;
	children: GroupNode[];
}

/**
 * A row in the entry list. Deliberately narrow: everything here is safe to hold
 * in JS memory for as long as the list is on screen.
 */
export interface EntrySummary {
	uuid: Uuid;
	title: string;
	username: string;
	/** Host only, not the full URL — enough to disambiguate, less to leak. */
	urlHost: string;
	modified: Millis;
	/** Drives the row icon and its colour. */
	iconId: number;
	hasTotp: boolean;
	hasAttachments: boolean;
}

/**
 * A field on an entry. `value` is present only when `protected` is false.
 * This is the shape that keeps the rule honest: there is no variant of this
 * type that carries a protected value.
 */
export interface Field {
	key: string;
	protected: boolean;
	/** Absent for protected fields. Absent means "ask Rust", not "empty". */
	value?: string;
	/** True when the field exists but is the empty string. */
	empty: boolean;
}

export interface EntryDetail {
	uuid: Uuid;
	title: string;
	fields: Field[];
	tags: string[];
	times: {
		created: Millis;
		modified: Millis;
		accessed: Millis;
		expires: Millis | null;
	};
	attachments: { name: string; size: number | null }[];
	historyCount: number;
	groupPath: string[];
}

/** Progress during key derivation, so a ~1s Argon2 pass does not look like a hang. */
export type UnlockPhase =
	| { phase: "reading" }
	| {
			phase: "deriving" /** 0..1, or null when the KDF cannot report progress. */;
			progress: number | null;
	  }
	| { phase: "decrypting" }
	| { phase: "parsing" };

/**
 * One field change in an {@link Ipc.updateEntry} call.
 *
 * `value: null` removes the field. That is distinct from `""`, which means the
 * field exists and is empty.
 */
export interface FieldEdit {
	key: string;
	value: string | null;
	protected: boolean;
}

export interface GeneratorOptions {
	lowercase: boolean;
	uppercase: boolean;
	digits: boolean;
	symbols: boolean;
	/** Drop characters that are easy to misread when transcribing by hand. */
	excludeAmbiguous: boolean;
}

export interface SaveResult {
	bytesWritten: number;
	/** Filename of the rotated backup, if one was made. */
	backup: string | null;
}

export type UnlockError =
	| { kind: "wrong-password" }
	| { kind: "not-found"; path: string }
	| { kind: "corrupt"; detail: string }
	| { kind: "unsupported"; detail: string }
	| { kind: "io"; detail: string };

export interface Ipc {
	openVault(
		path: string,
		password: string,
		onProgress?: (p: UnlockPhase) => void,
	): Promise<VaultInfo>;
	lock(): Promise<void>;

	groups(): Promise<GroupNode>;
	listEntries(opts: { group?: Uuid; query?: string }): Promise<EntrySummary[]>;
	getEntry(uuid: Uuid): Promise<EntryDetail>;

	/**
	 * Copy a field to the clipboard. The secret goes Rust → clipboard directly
	 * and is never returned here. Resolves with the number of seconds until the
	 * clipboard is automatically cleared.
	 */
	copyField(uuid: Uuid, key: string): Promise<{ clearAfterSeconds: number }>;

	/**
	 * Return a protected value in plaintext.
	 *
	 * The one place a secret enters JS, and only on an explicit user action —
	 * clicking "reveal". Unavoidable if a password is to be displayed at all:
	 * rendering it Rust-side would mean drawing text outside the WebView.
	 *
	 * Mitigations, all of which the caller must honour:
	 *   - never store the result outside the revealing component's state
	 *   - clear on unmount, on lock, and on auto-lock
	 *   - never log it, never put it in an error, never send it back over IPC
	 */
	revealField(uuid: Uuid, key: string): Promise<string>;

	/** Current TOTP code plus seconds remaining in the window. */
	totp(
		uuid: Uuid,
	): Promise<{ code: string; secondsRemaining: number; period: number }>;

	/** Ask the OS for a file, since the WebView cannot see the filesystem. */
	pickVaultFile(): Promise<string | null>;

	/** Most recently opened vault paths, for the unlock screen. */
	recentVaults(): Promise<string[]>;

	// --- writing ---

	/**
	 * Apply a set of field changes to one entry.
	 *
	 * The whole edit is one call so the core can push exactly one history
	 * record; a per-field call would archive a copy per keystroke. Resolves
	 * with whether anything actually changed.
	 */
	updateEntry(uuid: Uuid, fields: FieldEdit[]): Promise<boolean>;

	createEntry(group: Uuid): Promise<Uuid>;

	/** Resolves with what actually happened — recycled or purged. */
	deleteEntry(uuid: Uuid): Promise<"recycled" | "purged">;

	moveEntry(uuid: Uuid, group: Uuid): Promise<void>;

	/**
	 * Write the vault to disk.
	 *
	 * The core verifies the encoded bytes re-open before replacing anything, so
	 * a rejected save means the file on disk is untouched.
	 */
	saveVault(): Promise<SaveResult>;

	/** Whether there are edits not yet written to disk. */
	isDirty(): Promise<boolean>;

	/**
	 * Generate a password.
	 *
	 * In the core rather than here: `Math.random()` is not cryptographically
	 * secure, and browser-side generators have shipped predictable passwords
	 * because of it.
	 */
	generatePassword(length: number, options: GeneratorOptions): Promise<string>;
}
