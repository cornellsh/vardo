/**
 * In-browser mock of the Rust core, so the UI can be built and reviewed without
 * the Tauri shell (which needs webkit2gtk-4.1 on Linux).
 *
 * The data is deliberately *messy*. `DESIGN.md` says to design against real
 * content, and a vault that only contains tidy 12-character titles will hide
 * every layout bug worth finding. So this generates missing usernames, 70-char
 * titles, CJK, emoji, duplicate names, ancient timestamps and empty groups.
 */

import type {
	EntryDetail,
	EntrySummary,
	ConflictPolicy,
	FieldEdit,
	GeneratorOptions,
	GroupNode,
	Ipc,
	SaveResult,
	SyncOutcome,
	SyncPreview,
	UnlockPhase,
	Uuid,
	VaultInfo,
} from "./types";

const DEMO_PASSWORD = "demopass";

/** Matches what `storage_risk::assess` reports for a real rclone mount. */
const MOCK_STORAGE_WARNING = {
	kind: "fuse",
	explanation:
		'This vault is on a userspace mount (rclone). Saving relies on an atomic rename, which userspace mounts often cannot provide — the usual result is a forked copy like "vault (1).kdbx" or a save that silently lands on a stale version.',
	remedy:
		"Move the vault to ordinary local storage and let Vardo sync it through the provider's API instead. Vardo can then merge properly rather than racing a background daemon.",
} as const;

/**
 * Deterministic PRNG, so the mock vault is identical between reloads.
 *
 * `Math.imul` is defensive rather than required: the plain `s * 1664525` form
 * peaks at 7.15e15, which is under 2^53, so it does not actually lose
 * precision. Measured both ways over 4120 draws into 37 buckets and the
 * distributions are identical. `imul` is kept only because it is correct
 * without needing that margin argument.
 */
function rng(seed: number): () => number {
	let s = seed >>> 0;
	return () => {
		s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
		return s / 0x1_0000_0000;
	};
}

const rand = rng(0x5eed);
const pick = <T>(xs: readonly T[]): T =>
	xs[Math.floor(rand() * xs.length)] as T;

const SERVICES = [
	"GitHub",
	"GitLab",
	"Fastmail",
	"Proton Mail",
	"Hetzner",
	"DigitalOcean",
	"Cloudflare",
	"AWS",
	"Namecheap",
	"Stripe",
	"Revolut",
	"Monzo",
	"Deutsche Bank",
	"Steam",
	"GOG",
	"Bandcamp",
	"Discogs",
	"Spotify",
	"Netflix",
	"Criterion",
	"Bahn.de",
	"Lufthansa",
	"Booking.com",
	"Airbnb",
	"Zipcar",
	"Bundesagentur für Arbeit",
	"Finanzamt ELSTER",
	"Krankenkasse TK",
	"国民健康保険",
	"楽天カード",
	"Госуслуги",
	"Old Router (192.168.1.1)",
	"NAS admin",
	"Synology DSM",
	"Home Assistant",
	"university library — interlibrary loan renewal portal (old, may be dead)",
	"Mum’s laptop 💻",
	"shared family Netflix 🍿",
];

const DOMAINS: Record<string, string> = {
	GitHub: "github.com",
	GitLab: "gitlab.com",
	Fastmail: "fastmail.com",
	"Proton Mail": "account.proton.me",
	Hetzner: "console.hetzner.cloud",
	Cloudflare: "dash.cloudflare.com",
	AWS: "console.aws.amazon.com",
	Stripe: "dashboard.stripe.com",
	Steam: "store.steampowered.com",
	Spotify: "spotify.com",
	Netflix: "netflix.com",
	"Bahn.de": "bahn.de",
};

const USERNAMES = [
	"alex@fastmail.com",
	"alex.mercer",
	"a.mercer+shopping@fastmail.com",
	"admin",
	"root",
	"",
	"",
	"alex@work.example",
	"0176 3344 1122",
];

interface MockEntry {
	summary: EntrySummary;
	password: string;
	notes: string;
	totp: boolean;
	group: Uuid;
	/** Non-standard fields, so custom-attribute editing is exercised. */
	custom: Record<string, { value: string; protected: boolean }>;
	historyCount: number;
}

const GROUPS: { uuid: Uuid; name: string }[] = [
	{ uuid: "g-root", name: "passwords" },
	{ uuid: "g-email", name: "Email" },
	{ uuid: "g-finance", name: "Finance" },
	{ uuid: "g-infra", name: "Infrastructure" },
	{ uuid: "g-shopping", name: "Shopping" },
	{ uuid: "g-gov", name: "Government & Admin" },
	{ uuid: "g-media", name: "Media" },
	{ uuid: "g-old", name: "Archive (pre-2015)" },
	// The last two are excluded from entry assignment, so both stay empty.
	{ uuid: "g-bin", name: "Recycle Bin" },
	{ uuid: "g-empty", name: "Work — new job" },
];

const NOW = Date.UTC(2026, 7, 26);

function makeEntries(count: number): MockEntry[] {
	const out: MockEntry[] = [];
	for (let i = 0; i < count; i++) {
		const service = pick(SERVICES);
		// Duplicates are realistic and expose truncation bugs in the list.
		const title = rand() < 0.12 ? `${service} (old)` : service;
		// Excludes g-empty and the bin: an empty group is a case the UI has to
		// handle, so the mock must actually produce one.
		const group = pick(GROUPS.slice(1, -2)).uuid;
		const ageDays = group === "g-old" ? 3000 + rand() * 2000 : rand() * 1400;
		out.push({
			summary: {
				uuid: `e-${i}`,
				title,
				username: pick(USERNAMES),
				urlHost:
					DOMAINS[service] ??
					(rand() < 0.4
						? ""
						: `${service.toLowerCase().replace(/[^a-z]/g, "")}.com`),
				modified: NOW - ageDays * 86_400_000,
				iconId: Math.floor(rand() * 68),
				hasTotp: rand() < 0.18,
				hasAttachments: rand() < 0.08,
			},
			password: "correct-horse-battery-staple",
			notes:
				rand() < 0.3
					? "Recovery codes are in the attachment.\nSecond line, to check wrapping."
					: "",
			totp: rand() < 0.18,
			group,
			custom:
				rand() < 0.15
					? { "Security question": { value: "first pet", protected: false } }
					: {},
			historyCount: Math.floor(rand() * 6),
		});
	}
	return out;
}

const ENTRIES = makeEntries(412);

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class MockIpc implements Ipc {
	private unlocked = false;
	private dirty = false;

	async openVault(
		path: string,
		password: string,
		onProgress?: (p: UnlockPhase) => void,
	): Promise<VaultInfo> {
		onProgress?.({ phase: "reading" });
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

		onProgress?.({ phase: "decrypting" });
		await delay(90);

		if (password !== DEMO_PASSWORD) {
			throw { kind: "wrong-password" };
		}

		onProgress?.({ phase: "parsing" });
		await delay(140);

		this.unlocked = true;
		return {
			name: "passwords",
			path,
			entryCount: ENTRIES.length,
			format: "KDBX 4.0",
			writable: true,
			// The reference vault really is on an rclone mount, so the mock
			// reproduces the warning rather than the tidy case.
			storageWarning: MOCK_STORAGE_WARNING,
		};
	}

	async lock(): Promise<void> {
		this.unlocked = false;
	}

	private assertUnlocked() {
		if (!this.unlocked) throw new Error("vault is locked");
	}

	async vaultInfo(): Promise<VaultInfo> {
		this.assertUnlocked();
		return {
			name: "passwords",
			path: "/home/user/GoogleDrive/passwords.kdbx",
			storageWarning: MOCK_STORAGE_WARNING,
			entryCount: ENTRIES.length,
			format: "KDBX 4.0",
			writable: true,
		};
	}

	async groups(): Promise<GroupNode> {
		this.assertUnlocked();
		const countIn = (uuid: Uuid) =>
			ENTRIES.filter((e) => e.group === uuid).length;
		return {
			uuid: "g-root",
			name: "passwords",
			entryCount: ENTRIES.length,
			children: GROUPS.slice(1).map((g) => ({
				uuid: g.uuid,
				name: g.name,
				entryCount: countIn(g.uuid),
				children: [],
			})),
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

	async getEntry(uuid: Uuid): Promise<EntryDetail> {
		this.assertUnlocked();
		const e = ENTRIES.find((x) => x.summary.uuid === uuid);
		if (!e) throw new Error(`no such entry: ${uuid}`);
		const groupName = GROUPS.find((g) => g.uuid === e.group)?.name ?? "?";
		return {
			uuid,
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
			tags: e.group === "g-old" ? ["archive"] : [],
			times: {
				created: e.summary.modified - 86_400_000 * 30,
				modified: e.summary.modified,
				accessed: e.summary.modified,
				expires: null,
			},
			attachments: e.summary.hasAttachments
				? [{ name: "recovery-codes.txt", size: 412 }]
				: [],
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

	// --- sync ---

	async syncPreview(): Promise<SyncPreview> {
		this.assertUnlocked();
		await delay(700);
		// A fixed, deliberately awkward plan: one of everything the review screen
		// has to render, including the two conflict kinds that are easiest to
		// explain badly.
		return {
			remote: "passwords.kdbx (local file)",
			remoteUnchanged: false,
			firstSync: false,
			changes: [
				{
					kind: "entry-added",
					title: "Hetzner",
					detail: "Added in Infrastructure by the other device",
					side: "remote",
				},
				{
					kind: "entry-updated",
					title: "Deutsche Bank",
					detail:
						"The other device wins for Password and URL. The other version is kept in history.",
					side: "remote",
				},
				{
					kind: "entry-updated",
					title: "GitHub",
					detail:
						"This device wins for Notes. The other version is kept in history.",
					side: "local",
				},
				{
					kind: "entry-deleted",
					title: "Old Router (192.168.1.1)",
					detail: "Deleted by the other device",
					side: "remote",
				},
				{
					kind: "entry-moved",
					title: "Spotify",
					detail: "Moved to Media by this device",
					side: "local",
				},
				{
					kind: "group-added",
					title: "Work — new job",
					detail: "Group added by the other device",
					side: "remote",
				},
			],
			conflicts: [
				{
					kind: "concurrent-edit",
					title: "Proton Mail",
					detail:
						"Both devices changed Password in the same second, so neither is newer.",
				},
				{
					kind: "delete-versus-edit",
					title: "Krankenkasse TK",
					detail:
						"The other device deleted this in the same second this device edited it.",
				},
			],
		};
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

	async totp(): Promise<{
		code: string;
		secondsRemaining: number;
		period: number;
	}> {
		this.assertUnlocked();
		const period = 30;
		const secondsRemaining = period - (Math.floor(Date.now() / 1000) % period);
		const code = String(Math.floor(rand() * 1_000_000)).padStart(6, "0");
		return { code, secondsRemaining, period };
	}

	async pickVaultFile(): Promise<string | null> {
		return "/home/user/GoogleDrive/passwords.kdbx";
	}

	async recentVaults(): Promise<string[]> {
		return ["/home/user/GoogleDrive/passwords.kdbx"];
	}
}
