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
	GroupNode,
	Ipc,
	UnlockPhase,
	Uuid,
	VaultInfo,
} from "./types";

const DEMO_PASSWORD = "demopass";

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
		});
	}
	return out;
}

const ENTRIES = makeEntries(412);

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class MockIpc implements Ipc {
	private unlocked = false;

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
		};
	}

	async lock(): Promise<void> {
		this.unlocked = false;
	}

	private assertUnlocked() {
		if (!this.unlocked) throw new Error("vault is locked");
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
			historyCount: Math.floor(rand() * 6),
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
		if (key !== "Password") throw new Error(`not a protected field: ${key}`);
		return e.password;
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
