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

// Every type Rust sends or receives is generated from it
// (`inv:ipc-types-are-generated`). Edit the Rust type, then regenerate.
import type {
	Attachment,
	ChangeView,
	ConflictPolicy,
	ConflictView,
	CopyResult,
	CoreError,
	DriveStatus,
	DriveVault,
	EntryDetail,
	EntrySummary,
	Field,
	FieldEdit,
	Finding,
	GeneratorOptions,
	GroupNode,
	HealthReport,
	HistoryVersion,
	Issue,
	Phase,
	SaveResult,
	StorageKind,
	StorageWarning,
	SyncOutcome,
	SyncOutcomeKind,
	SyncPreview,
	SyncWarning,
	SyncWarningKind,
	Times,
	TotpCode,
	UnlockPhase,
	VaultInfo,
	VaultSource,
} from "./bindings";

export type {
	Attachment,
	ChangeView,
	ConflictPolicy,
	ConflictView,
	CopyResult,
	CoreError,
	DriveStatus,
	DriveVault,
	EntryDetail,
	EntrySummary,
	Field,
	FieldEdit,
	Finding,
	GeneratorOptions,
	GroupNode,
	HealthReport,
	HistoryVersion,
	Issue,
	Phase,
	SaveResult,
	StorageKind,
	StorageWarning,
	SyncOutcome,
	SyncOutcomeKind,
	SyncPreview,
	SyncWarning,
	SyncWarningKind,
	Times,
	TotpCode,
	UnlockPhase,
	VaultInfo,
	VaultSource,
};

/** The errors an unlock can end in. */
export type UnlockError = Extract<
	CoreError,
	{ kind: "wrong-password" | "not-found" | "corrupt" | "unsupported" | "io" }
>;

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

	/** Reused, weak, expired and stale passwords. Never includes a password. */
	health(): Promise<HealthReport>;

	/** Earlier versions of an entry, newest first. Never includes a password. */
	history(uuid: Uuid): Promise<HistoryVersion[]>;

	/**
	 * Whether a string would produce one-time codes.
	 *
	 * Runs the parser that will later read it back, so the answer cannot drift
	 * from what actually happens. No session needed: it is a question about a
	 * string.
	 */
	checkTotpSeed(text: string): Promise<boolean>;

	// --- groups ---

	createGroup(parent: Uuid, name: string): Promise<Uuid>;
	renameGroup(uuid: Uuid, name: string): Promise<void>;
	/** Removes a group and everything in it. Returns how many entries went. */
	deleteGroup(uuid: Uuid): Promise<number>;
	/** For the confirmation that precedes deleting one. */
	groupEntryCount(uuid: Uuid): Promise<number>;

	/**
	 * Sets or clears an entry's expiry.
	 *
	 * Null clears the flag and leaves the stored date alone, matching the vault
	 * layer and every other client: the date is the flag's argument, not a
	 * second source of truth.
	 */
	setExpiry(uuid: Uuid, atUnix: number | null): Promise<void>;

	setTags(uuid: Uuid, tags: string[]): Promise<void>;

	/**
	 * Attaches a file. Returns the name it was stored under.
	 *
	 * Takes a path, not bytes. Rust reads the file, so a recovery-codes
	 * document never becomes a JavaScript string sitting in the WebView's heap
	 * for as long as the page lives.
	 */
	addAttachment(uuid: Uuid, path: string): Promise<string>;

	removeAttachment(uuid: Uuid, name: string): Promise<void>;

	/** A file chooser. Any type: a vault holds whatever sits beside a password. */
	pickAnyFile(): Promise<string | null>;

	// --- system tray ---
	//
	// Optional, because the browser build has no tray. Left optional rather than
	// stubbed in the mock, so every caller has to acknowledge it may be absent
	// instead of quietly relying on a no-op that only exists in one build.

	/**
	 * Tells the tray whether the vault is open.
	 *
	 * The window can be hidden while the vault is unlocked, and the icon is then
	 * the only thing saying so. An icon that lags reality would claim locked
	 * while every password was one click away.
	 */
	setTrayUnlocked?(unlocked: boolean): Promise<void>;

	/** The tray's "Lock now". Resolves to a function that stops listening. */
	onLockRequested?(handler: () => void): Promise<() => void>;

	/**
	 * The vault changed outside the window: a login saved from the browser
	 * extension. Resolves to a function that stops listening.
	 */
	onVaultChanged?(handler: () => void): Promise<() => void>;

	/** The quick-access shortcut opened the quick window again. */
	onQuickOpen?(handler: () => void): Promise<() => void>;

	/** Hides the quick window, and shows the main one when `revealMain`. */
	quickDone?(revealMain: boolean): Promise<void>;

	/** Copies an old password, cleared on the same schedule as any other. */
	copyHistoryPassword(
		uuid: Uuid,
		index: number,
	): Promise<{ clearAfterSeconds: number }>;

	/**
	 * Writes an attachment to a path the user chose. Returns the byte count,
	 * so the UI can confirm the file is really there rather than assume.
	 */
	saveAttachment(uuid: Uuid, name: string, dest: string): Promise<number>;

	/** A save dialog. Null when the user cancels. */
	pickSavePath(suggested: string): Promise<string | null>;

	/** Drops a vault from recents without touching the vault. */
	forgetVault(source: VaultSource): Promise<void>;
}
