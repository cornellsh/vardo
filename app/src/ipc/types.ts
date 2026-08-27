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

/**
 * Present when the vault lives somewhere that can corrupt it — a userspace
 * mount, a network mount, or a directory a sync daemon watches.
 *
 * Reported at unlock rather than at save, because the first save is already too
 * late.
 */
export interface StorageWarning {
	kind: "fuse" | "network" | "name";
	explanation: string;
	remedy: string;
	/** The full reasoning, shown only if the user opens it. */
	why: string;
}

export interface VaultInfo {
	name: string;
	/** The vault's short name. Not a path: a Drive vault has none. */
	path: string;
	/** Where it lives, in words. "Google Drive", or the containing folder. */
	location: string;
	source: VaultSource;
	/** Only ever set for a local file. A Drive vault is on no mount at all. */
	storageWarning: StorageWarning | null;
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
	/** A PNG the vault already carries, as a data URL. No network, no leak. */
	iconPng: string | null;
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
	/** The vault's own icon for this entry, as a data URL. */
	iconPng: string | null;
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

/**
 * Where a vault lives.
 *
 * Mirrors the Rust enum, tag and all. A vault is not a path: that assumption is
 * what made "open from Google Drive" impossible to express in the first place.
 */
export type VaultSource =
	| { kind: "file"; path: string }
	| { kind: "googleDrive"; fileId: string; name: string };

/** A vault found in the user's Drive. */
export interface DriveVault {
	fileId: string;
	name: string;
	size: number;
	modifiedUnix: number | null;
}

/**
 * Whether Drive can be used at all, and whether it currently is.
 *
 * Two flags rather than one: "this build ships no client id" and "you have not
 * signed in" are different problems, and only one of them is the user's to fix.
 */
export interface DriveStatus {
	configured: boolean;
	connected: boolean;
}

export interface TotpCode {
	code: string;
	secondsRemaining: number;
	period: number;
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
		source: VaultSource,
		password: string,
		onProgress?: (p: UnlockPhase) => void,
	): Promise<VaultInfo>;
	lock(): Promise<void>;

	/**
	 * Current vault metadata. Re-read after a merge, which rewrites the vault in
	 * place and invalidates the entry count handed out at unlock.
	 */
	vaultInfo(): Promise<VaultInfo>;

	groups(): Promise<GroupNode>;
	/**
	 * Ordering for a listing.
	 *
	 * Two, not five. Alphabetical finds something you know the name of; recent
	 * finds what you were just doing. Every other axis anyone proposes turns
	 * out to be one of those two.
	 */
	listEntriesSorted(
		group: string | null,
		query: string | null,
		sort: "name" | "recent",
	): Promise<EntrySummary[]>;

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

	/**
	 * The current one-time code, or `null` when the entry has no seed.
	 *
	 * Only the derived code crosses the boundary. The seed is a long-lived
	 * shared secret — strictly more valuable than any single code, because it
	 * generates every future one — and stays in the core.
	 */
	totp(uuid: Uuid): Promise<TotpCode | null>;

	/** Ask the OS for a file, since the WebView cannot see the filesystem. */
	pickVaultFile(): Promise<string | null>;

	/** Most recently opened vault paths, for the unlock screen. */
	recentVaults(): Promise<VaultSource[]>;

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

	// --- sync ---

	/**
	 * Report what a merge would do, without touching anything.
	 *
	 * `vardo_merge::plan` is pure precisely so this is possible: the user sees
	 * the consequences before any password changes.
	 */
	syncPreview(): Promise<SyncPreview>;

	/** Perform the sync. `policy` decides anything the plan could not resolve. */
	syncApply(policy: ConflictPolicy): Promise<SyncOutcome>;

	// --- Google Drive ---

	driveStatus(): Promise<DriveStatus>;

	/**
	 * Run the OAuth flow. Resolves once the browser has come back, so it is
	 * long-running by nature and the caller must show that it is waiting.
	 */
	driveConnect(): Promise<void>;

	driveDisconnect(): Promise<void>;

	/**
	 * Vaults in Drive that Vardo may see. Empty is a normal answer, not a
	 * failure, because the `drive.file` scope hides everything Vardo has not
	 * been given.
	 */
	driveListVaults(): Promise<DriveVault[]>;

	/** Copies a local vault into Drive. The original is left alone. */
	driveUploadVault(path: string): Promise<VaultSource>;

	/**
	 * Opens Google's own file picker.
	 *
	 * The only way to reach a vault Vardo did not create without asking for
	 * access to the user's whole Drive. Resolves to null if they close it.
	 */
	drivePickVault(): Promise<VaultSource | null>;

	/**
	 * Downloads an entry's site icon and stores it in the vault.
	 *
	 * Direct from the site, never an aggregator, and written into the vault so
	 * it happens once ever rather than once per device. Null when the site has
	 * no usable icon, which is ordinary.
	 */
	fetchIcon(uuid: Uuid): Promise<string | null>;

	/** Entries with a web address and no icon yet. */
	entriesMissingIcons(): Promise<Uuid[]>;

	/** Drops a vault from recents without touching the vault. */
	forgetVault(source: VaultSource): Promise<void>;
}

// --- sync ---

export interface ChangeView {
	kind: string;
	title: string;
	detail: string;
	side: string | null;
}

export interface ConflictView {
	kind: string;
	title: string;
	detail: string;
}

/**
 * Something that does not stop the sync but changes what it will do.
 *
 * Distinct from a conflict: every one of these describes a sync that will
 * still happen, but will behave differently from what the user expects unless
 * they are told.
 */
export interface SyncWarning {
	kind: "no-base" | "remote-read-only" | "clock-skew";
	detail: string;
}

export interface SyncPreview {
	warnings: SyncWarning[];
	remote: string;
	/** Remote is byte-identical to our sync point: nothing to fold in. */
	remoteUnchanged: boolean;
	/**
	 * No merge base yet. Without one, an addition on one side is
	 * indistinguishable from a deletion on the other, so nothing is deleted.
	 */
	firstSync: boolean;
	changes: ChangeView[];
	conflicts: ConflictView[];
}

export type ConflictPolicy =
	| "duplicate"
	| "keep-local"
	| "keep-remote"
	| "refuse";

export interface SyncOutcome {
	kind: "up-to-date" | "pushed" | "merged" | "needs-review" | "created";
	detail: string;
	racesLost: number;
	retries: number;
}
