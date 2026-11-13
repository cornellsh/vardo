/**
 * In-browser mock of the Rust core, so the UI can be built and reviewed without
 * the Tauri shell (which needs webkit2gtk-4.1 on Linux).
 *
 * The data is deliberately *messy*. Design against real content, because a vault that only contains tidy 12-character titles will hide
 * every layout bug worth finding. So this generates missing usernames, 70-char
 * titles, CJK, emoji, duplicate names, ancient timestamps and empty groups.
 */

import type {
	BrowserHosts,
	DriveVault,
	VaultSource,
	EntryDetail,
	EntrySummary,
	ConflictPolicy,
	DriveStatus,
	FieldEdit,
	GeneratorOptions,
	GroupNode,
	Ipc,
	Finding,
	HealthReport,
	HistoryVersion,
	SaveResult,
	SyncOutcome,
	SyncPreview,
	TotpCode,
	UnlockPhase,
	Uuid,
	VaultInfo,
} from "./types";
import { sourceKey, sourceName, webHost } from "../lib/source";
import { DEMO_PASSWORD, GROUPS, ENTRIES, storageWarningFor, DEMO_SYNC_PLAN } from "./mockData";

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class MockIpc implements Ipc {
	private unlocked = false;
	private dirty = false;
	private driveConnected = false;
	private source: VaultSource | null = null;
	private recents: VaultSource[] = [
		{ kind: "file", path: "/vaults/personal.kdbx" },
		{ kind: "file", path: "/vaults/work.kdbx" },
	];
	/**
	 * Starts empty on purpose. Under the `drive.file` scope a connected account
	 * with no app-created files is the *expected* first state, and it is the
	 * one whose copy is hardest to get right, so the mock must show it by
	 * default rather than as an edge case someone has to go looking for.
	 */
	private driveVaults: DriveVault[] = [];

	async openVault(
		source: VaultSource,
		password: string,
		onProgress?: (p: UnlockPhase) => void,
	): Promise<VaultInfo> {
		this.source = source;
		onProgress?.({ phase: "reading", progress: null });
		await delay(120);

		// Mirrors the measured 0.871s Argon2d pass on the real vault, because the
		// unlock screen has to be designed against the real duration, not an
		// instant mock that makes the progress UI look unnecessary.
		//
		// `progress: null` is deliberate and matches reality: the argon2 crate
		// exposes no per-pass callback, so the core can report which *phase* it is
		// in but not how far through the derivation it is. A mock that reported a
		// smooth fraction would have let us design a determinate bar we cannot
		// actually drive.
		onProgress?.({ phase: "deriving", progress: null });
		await delay(871);

		onProgress?.({ phase: "decrypting", progress: null });
		await delay(90);

		if (password !== DEMO_PASSWORD) {
			throw { kind: "wrong-password" };
		}

		onProgress?.({ phase: "parsing", progress: null });
		await delay(140);

		this.unlocked = true;
		return this.vaultInfo();
	}

	async lock(): Promise<void> {
		this.unlocked = false;
	}

	private assertUnlocked() {
		if (!this.unlocked) throw new Error("vault is locked");
	}

	async vaultInfo(): Promise<VaultInfo> {
		this.assertUnlocked();
		const source = this.source ?? {
			kind: "file" as const,
			path: "/vaults/demo.kdbx",
		};
		const folder = (path: string) =>
			path.split("/").slice(0, -1).join("/") || "/";

		return {
			name: sourceName(source),
			path: sourceName(source),
			// Derived from the actual source, not fixed strings. The old version
			// reported "/vaults" no matter which vault was open, so opening a
			// different one changed nothing on screen.
			location:
				source.kind === "file"
					? folder(source.path)
					: source.kind === "webDav"
						? webHost(source.url)
						: "Google Drive",
			source,
			// Only a local file can sit in someone else's synced folder. A vault
			// reached through Drive's own API is not in a folder we can see, and
			// warning about one that does not exist is how people learn to
			// dismiss warnings without reading them.
			storageWarning:
				source.kind === "file" ? storageWarningFor(source.path) : null,
			openElsewhere: false,
			entryCount: ENTRIES.length,
			format: "KDBX 4.0",
			writable: true,
		};
	}

	async groups(): Promise<GroupNode> {
		this.assertUnlocked();
		const countIn = (uuid: Uuid) =>
			ENTRIES.filter((e) => e.group === uuid).length;
		const build = (parent: Uuid): GroupNode[] =>
			GROUPS.filter((g) => g.parent === parent).map((g) => ({
				uuid: g.uuid,
				name: g.name,
				entryCount: countIn(g.uuid),
				children: build(g.uuid),
			}));

		return {
			uuid: "g-root",
			name: "passwords",
			entryCount: ENTRIES.length,
			children: build("g-root"),
		};
	}

	async listEntries({
		group,
		query,
	}: {
		group?: Uuid;
		query?: string;
	}): Promise<EntrySummary[]> {
		this.assertUnlocked();
		let rows = ENTRIES;
		if (group && group !== "g-root")
			rows = rows.filter((e) => e.group === group);
		if (query?.trim()) {
			const q = query.trim().toLowerCase();
			rows = rows.filter(
				(e) =>
					e.summary.title.toLowerCase().includes(q) ||
					e.summary.username.toLowerCase().includes(q) ||
					e.summary.urlHost.toLowerCase().includes(q),
			);
		}
		return rows
			.map((e) => e.summary)
			.sort(
				(a, b) => a.title.localeCompare(b.title) || b.modified - a.modified,
			);
	}

	async listEntriesSorted(
		group: string | null,
		query: string | null,
		sort: "name" | "recent",
	): Promise<EntrySummary[]> {
		const rows = await this.listEntries({
			...(group ? { group } : {}),
			...(query ? { query } : {}),
		});
		if (sort === "name") return rows;
		// Name breaks the tie, because an imported vault gives every entry the
		// same timestamp and an unstable sort would reshuffle on every keystroke.
		return [...rows].sort(
			(a, b) => b.modified - a.modified || a.title.localeCompare(b.title),
		);
	}

	async getEntry(uuid: Uuid): Promise<EntryDetail> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) throw new Error(`no such entry: ${uuid}`);
		const groupName = GROUPS.find((g) => g.uuid === e.group)?.name ?? "?";
		return {
			uuid,
			iconPng: null,
			title: e.summary.title,
			// Note the shape: Password carries no `value`. That is the contract.
			fields: [
				{
					key: "UserName",
					protected: false,
					value: e.summary.username,
					empty: !e.summary.username,
				},
				{ key: "Password", protected: true, empty: false },
				{
					key: "URL",
					protected: false,
					value: e.summary.urlHost ? `https://${e.summary.urlHost}` : "",
					empty: !e.summary.urlHost,
				},
				{ key: "Notes", protected: false, value: e.notes, empty: !e.notes },
				...Object.entries(e.custom).map(([key, f]) => ({
					key,
					protected: f.protected,
					...(f.protected ? {} : { value: f.value }),
					empty: !f.value,
				})),
			],
			// Reads back what was set, so editing tags or expiry shows a result
			// rather than reverting on the next read - which looks exactly like
			// the write having failed.
			tags: e.tags ?? (e.group === "g-old" ? ["archive"] : []),
			times: {
				created: e.summary.modified - 86_400_000 * 30,
				modified: e.summary.modified,
				accessed: e.summary.modified,
				expires: e.expires ?? null,
			},
			// Reflects what was attached and detached in this session. A list
			// that ignored both would make every attach look like it failed.
			attachments: [
				...(e.summary.hasAttachments
					? [{ name: "recovery-codes.txt", size: 412 }]
					: []),
				...(e.extraAttachments ?? []),
			].filter((a) => !(e.removedAttachments ?? []).includes(a.name)),
			// Stored, not re-rolled: a count that changes on every read would make
			// the detail pane flicker and hide real history bugs.
			historyCount: e.historyCount,
			groupPath: ["passwords", groupName],
		};
	}

	async copyField(): Promise<{ clearAfterSeconds: number }> {
		this.assertUnlocked();
		// The real implementation never returns the secret; neither does the mock,
		// so the UI cannot accidentally grow a dependency on getting one back.
		return { clearAfterSeconds: 15 };
	}

	async revealField(uuid: Uuid, key: string): Promise<string> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) throw new Error(`no such entry: ${uuid}`);
		if (key === "Password") return e.password;
		const custom = e.custom[key];
		if (custom?.protected) return custom.value;
		throw new Error(`not a protected field: ${key}`);
	}

	// --- writing ---

	async updateEntry(uuid: Uuid, fields: FieldEdit[]): Promise<boolean> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) throw new Error(`no such entry: ${uuid}`);

		let changed = false;
		const apply = <T>(current: T, next: T): T => {
			if (current !== next) changed = true;
			return next;
		};

		for (const f of fields) {
			if (f.value === null) {
				if (f.key in e.custom) {
					delete e.custom[f.key];
					changed = true;
				}
				continue;
			}
			switch (f.key) {
				case "Title":
					e.summary.title = apply(e.summary.title, f.value);
					break;
				case "UserName":
					e.summary.username = apply(e.summary.username, f.value);
					break;
				case "Password":
					e.password = apply(e.password, f.value);
					break;
				case "URL":
					e.summary.urlHost = apply(
						e.summary.urlHost,
						f.value.replace(/^https?:\/\//, "").split("/")[0] ?? "",
					);
					break;
				case "Notes":
					e.notes = apply(e.notes, f.value);
					break;
				default: {
					const before = e.custom[f.key];
					if (before?.value !== f.value || before?.protected !== f.protected) {
						changed = true;
					}
					e.custom[f.key] = { value: f.value, protected: f.protected };
				}
			}
		}

		// Mirrors the core: one history record per edit, and none at all when
		// nothing actually changed.
		if (changed) {
			e.historyCount += 1;
			e.summary.modified = Date.now();
			this.dirty = true;
		}
		return changed;
	}

	async createEntry(group: Uuid): Promise<Uuid> {
		this.assertUnlocked();
		const uuid = `e-new-${ENTRIES.length}`;
		ENTRIES.push({
			summary: {
				uuid,
				title: "",
				username: "",
				urlHost: "",
				modified: Date.now(),
				iconId: 0,
				iconPng: null,
				hasTotp: false,
				hasAttachments: false,
			},
			password: "",
			notes: "",
			totp: false,
			group: group === "g-root" ? "g-email" : group,
			custom: {},
			historyCount: 0,
		});
		this.dirty = true;
		return uuid;
	}

	async deleteEntry(uuid: Uuid): Promise<"recycled" | "purged"> {
		this.assertUnlocked();
		const at = ENTRIES.findIndex((x) => x.summary.uuid === uuid);
		if (at === -1) throw new Error(`no such entry: ${uuid}`);
		const entry = ENTRIES[at];
		if (!entry) throw new Error(`no such entry: ${uuid}`);
		this.dirty = true;

		// This mock vault has a recycle bin, so a first delete moves the entry
		// there and only a second one removes it.
		if (entry.group !== "g-bin") {
			entry.group = "g-bin";
			return "recycled";
		}
		ENTRIES.splice(at, 1);
		return "purged";
	}

	async moveEntry(uuid: Uuid, group: Uuid): Promise<void> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) throw new Error(`no such entry: ${uuid}`);
		e.group = group;
		this.dirty = true;
	}

	async saveVault(): Promise<SaveResult> {
		this.assertUnlocked();
		// The real save re-derives the key to verify its own output, so it is not
		// instant. Modelling that keeps the saving state visible in the UI.
		await delay(900);
		this.dirty = false;
		return { bytesWritten: 646_000, backup: "passwords.kdbx.1787770000.bak" };
	}

	async isDirty(): Promise<boolean> {
		return this.dirty;
	}

	// --- Google Drive ---

	async driveStatus(): Promise<DriveStatus> {
		// Configured but not connected, so the browser build shows the state
		// that actually needs designing.
		return { configured: true, connected: this.driveConnected };
	}

	async driveConnect(): Promise<void> {
		// Modelled slowly on purpose: the real flow waits for a round trip
		// through the system browser, and the UI has to look sane meanwhile.
		await delay(2500);
		this.driveConnected = true;
	}

	async driveDisconnect(): Promise<void> {
		await delay(200);
		this.driveConnected = false;
	}

	// --- WebDAV ---

	async webdavConnect(url: string, username: string, password: string): Promise<VaultSource> {
		await delay(400);
		if (!url.startsWith("https://")) throw { kind: "unsupported", detail: "Use https://. Plain http would send your password unencrypted." };
		if (password !== "dav") throw { kind: "io", detail: "not authorised: the WebDAV server refused these credentials (401)" };
		const source: VaultSource = { kind: "webDav", url, username };
		this.recents = [source, ...this.recents];
		return source;
	}

	// --- browser extension ---

	private browsersConnected = false;

	private browserReport(): BrowserHosts {
		return {
			helperFound: true,
			browsers: [
				{ name: "Google Chrome", installed: true, connected: this.browsersConnected },
				{ name: "Chromium", installed: false, connected: false },
				{ name: "Brave", installed: true, connected: this.browsersConnected },
			],
		};
	}

	async browserHosts(): Promise<BrowserHosts> {
		return this.browserReport();
	}

	async browserConnect(): Promise<BrowserHosts> {
		await delay(200);
		this.browsersConnected = true;
		return this.browserReport();
	}

	async browserDisconnect(): Promise<BrowserHosts> {
		await delay(200);
		this.browsersConnected = false;
		return this.browserReport();
	}

	// --- sync ---

	async syncPreview(): Promise<SyncPreview> {
		this.assertUnlocked();
		await delay(700);
		return structuredClone(DEMO_SYNC_PLAN);
	}

	async syncApply(policy: ConflictPolicy): Promise<SyncOutcome> {
		this.assertUnlocked();
		await delay(1200);
		this.dirty = false;
		if (policy === "refuse") {
			return {
				kind: "needs-review",
				detail: "2 item(s) need a decision. Nothing was uploaded.",
				racesLost: 0,
				retries: 0,
			};
		}
		return {
			kind: "merged",
			detail: "Merged and uploaded.",
			racesLost: 0,
			retries: 0,
		};
	}

	async generatePassword(
		length: number,
		options: GeneratorOptions,
	): Promise<string> {
		let alphabet = "";
		if (options.lowercase) alphabet += "abcdefghijklmnopqrstuvwxyz";
		if (options.uppercase) alphabet += "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
		if (options.digits) alphabet += "0123456789";
		if (options.symbols) alphabet += "!@#$%^&*()-_=+[]{};:,.?";
		if (options.excludeAmbiguous) {
			alphabet = [...alphabet]
				.filter((c) => !`0O1lI|\`'"`.includes(c))
				.join("");
		}
		if (!alphabet) throw new Error("no character classes selected");

		// crypto.getRandomValues, not Math.random. The real generator lives in
		// Rust; this exists only so the browser build works, and it must not
		// demonstrate the exact mistake the Rust one was written to avoid.
		const bytes = new Uint32Array(length);
		crypto.getRandomValues(bytes);
		return Array.from(
			bytes,
			(b) => alphabet[b % alphabet.length] as string,
		).join("");
	}

	async totp(uuid: Uuid): Promise<TotpCode | null> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e?.summary.hasTotp) return null;

		const period = 30;
		const secondsRemaining = period - (Math.floor(Date.now() / 1000) % period);
		// Derived from the counter so it actually changes on the boundary, which
		// is the behaviour the countdown ring is there to show.
		const counter = Math.floor(Date.now() / 1000 / period);
		const code = String((counter * 7919 + 13) % 1_000_000).padStart(6, "0");
		return { code, secondsRemaining, period };
	}

	async pickVaultFile(): Promise<string | null> {
		return "/vaults/from-picker.kdbx";
	}

	async recentVaults(): Promise<VaultSource[]> {
		return this.recents;
	}

	async forgetVault(source: VaultSource): Promise<void> {
		this.recents = this.recents.filter((s) => sourceKey(s) !== sourceKey(source));
	}

	async driveListVaults(): Promise<DriveVault[]> {
		await delay(600);
		if (!this.driveConnected) throw { kind: "drive-not-connected" };
		return this.driveVaults;
	}

	async fetchIcon(_uuid: Uuid): Promise<string | null> {
		await delay(700);
		// Nothing, which is what the browser build can actually fetch.
		//
		// This used to return a one-pixel red PNG "so the img tag is exercised".
		// It was never stored, so the list kept showing a letter while the header
		// showed a red square stretched to fill the avatar: a disagreement the real
		// app does not have, and the first thing anyone saw in a screenshot.
		return null;
	}

	async entriesMissingIcons(): Promise<Uuid[]> {
		return [];
	}

	async history(uuid: Uuid): Promise<HistoryVersion[]> {
		this.assertUnlocked();
		await delay(120);
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) return [];

		// Derived from the entry's own count, so an entry claiming two earlier
		// versions produces exactly two. A fixed number here would have made
		// the header and the list disagree, which is the sort of thing that
		// looks like a bug in the data rather than in the mock.
		const count = e.historyCount ?? 0;
		const day = 86_400_000;
		return Array.from({ length: count }, (_, i) => ({
			index: count - 1 - i,
			title: e.summary.title,
			username: e.summary.username,
			url: e.summary.urlHost ? `https://${e.summary.urlHost}` : "",
			modified: e.summary.modified - (i + 1) * 47 * day,
			hasPassword: true,
		}));
	}

	async copyHistoryPassword(): Promise<{ clearAfterSeconds: number }> {
		await delay(80);
		return { clearAfterSeconds: 12 };
	}

	async createGroup(parent: Uuid, name: string): Promise<Uuid> {
		this.assertUnlocked();
		const uuid = `g-new-${GROUPS.length}`;
		GROUPS.push({ uuid, name, parent });
		this.dirty = true;
		return uuid;
	}

	async renameGroup(uuid: Uuid, name: string): Promise<void> {
		this.assertUnlocked();
		const g = GROUPS.find((x) => x.uuid === uuid);
		if (g) g.name = name;
		this.dirty = true;
	}

	async deleteGroup(uuid: Uuid): Promise<number> {
		this.assertUnlocked();
		// Descendants too, matching the core. A mock that removed only the
		// named group would leave orphans the real thing never produces, and
		// the tree would look broken for reasons that are not in the product.
		const doomed = new Set<string>([uuid]);
		let grew = true;
		while (grew) {
			grew = false;
			for (const g of GROUPS) {
				if (g.parent && doomed.has(g.parent) && !doomed.has(g.uuid)) {
					doomed.add(g.uuid);
					grew = true;
				}
			}
		}
		const lost = ENTRIES.filter((e) => doomed.has(e.group)).length;
		for (let i = GROUPS.length - 1; i >= 0; i--) {
			const g = GROUPS[i];
			if (g && doomed.has(g.uuid)) GROUPS.splice(i, 1);
		}
		this.dirty = true;
		return lost;
	}

	async groupEntryCount(uuid: Uuid): Promise<number> {
		this.assertUnlocked();
		return ENTRIES.filter((e) => e.group === uuid).length;
	}

	async setExpiry(uuid: Uuid, atUnix: number | null): Promise<void> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (e) e.expires = atUnix === null ? null : atUnix * 1000;
		this.dirty = true;
	}

	async setTags(uuid: Uuid, tags: string[]): Promise<void> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (e) e.tags = tags;
		this.dirty = true;
	}

	async addAttachment(uuid: Uuid, path: string): Promise<string> {
		this.assertUnlocked();
		await delay(200);
		const name = path.split("/").pop() ?? "attachment";
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (e) {
			e.extraAttachments = [
				...(e.extraAttachments ?? []).filter((a) => a.name !== name),
				{ name, size: 2048 },
			];
		}
		this.dirty = true;
		return name;
	}

	async removeAttachment(uuid: Uuid, name: string): Promise<void> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (e) {
			e.extraAttachments = (e.extraAttachments ?? []).filter(
				(a) => a.name !== name,
			);
			e.removedAttachments = [...(e.removedAttachments ?? []), name];
		}
		this.dirty = true;
	}

	async pickAnyFile(): Promise<string | null> {
		return "/home/you/Documents/recovery-codes.txt";
	}

	async checkTotpSeed(text: string): Promise<boolean> {
		await delay(60);
		const t = text.trim();
		if (t.startsWith("otpauth://")) return t.includes("secret=");
		// Base32 with optional padding, and long enough to be a real secret.
		// Crude next to the Rust parser on purpose: the mock exists to exercise
		// both branches of the UI, not to be a second implementation that can
		// disagree with the first.
		return /^[A-Z2-7]{16,}=*$/i.test(t.replace(/\s+/g, ""));
	}

	async health(): Promise<HealthReport> {
		this.assertUnlocked();
		await delay(180);

		// Derived from the mock vault rather than invented, so the panel is
		// exercised against the same messy titles as everything else: long ones,
		// CJK, emoji, duplicates. A findings list of three tidy examples would
		// prove the layout works on data that does not exist.
		const rows = ENTRIES.map((e) => e.summary);
		const findings: Finding[] = [];

		const reused = rows.slice(0, 6);
		for (const r of reused) {
			findings.push({
				uuid: r.uuid,
				title: r.title,
				issue: "reused",
				sharedWith: 3,
			});
		}
		for (const r of rows.slice(10, 19)) {
			findings.push({
				uuid: r.uuid,
				title: r.title,
				issue: "weak",
				sharedWith: null,
			});
		}
		for (const r of rows.slice(30, 33)) {
			findings.push({
				uuid: r.uuid,
				title: r.title,
				issue: "expired",
				sharedWith: null,
			});
		}
		for (const r of rows.slice(40, 68)) {
			findings.push({
				uuid: r.uuid,
				title: r.title,
				issue: "stale",
				sharedWith: null,
			});
		}

		return { findings, checked: rows.length };
	}

	async saveAttachment(
		_uuid: Uuid,
		_name: string,
		_dest: string,
	): Promise<number> {
		await delay(300);
		return 4096;
	}

	async pickSavePath(suggested: string): Promise<string | null> {
		return `/home/you/Downloads/${suggested}`;
	}

	async drivePickVault(): Promise<VaultSource | null> {
		// Models the real thing: a round trip through the browser, then a file
		// that was not in the list before because Vardo could not see it.
		await delay(1800);
		const source: VaultSource = {
			kind: "googleDrive",
			file_id: "picked-1",
			name: "passwords.kdbx",
		};
		this.driveVaults.push({
			fileId: "picked-1",
			name: "passwords.kdbx",
			size: 646_000,
			modifiedUnix: Math.floor(Date.now() / 1000),
		});
		return source;
	}

	async driveUploadVault(path: string): Promise<VaultSource> {
		await delay(900);
		const name = path.split("/").pop() ?? "vault.kdbx";
		const source: VaultSource = {
			kind: "googleDrive",
			file_id: `mock-${this.driveVaults.length + 1}`,
			name,
		};
		this.driveVaults.push({
			fileId: source.kind === "googleDrive" ? source.file_id : "",
			name,
			size: 646_000,
			modifiedUnix: Math.floor(Date.now() / 1000),
		});
		this.recents.unshift(source);
		return source;
	}
}
