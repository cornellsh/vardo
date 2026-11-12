/**
 * The messy demo vault behind `MockIpc`: entries, groups and a sync plan.
 */

import type {
	EntrySummary,
	StorageWarning,
	SyncPreview,
	Uuid,
} from "./types";

export const DEMO_PASSWORD = "demopass";

/**
 * Mirrors `storage_risk::assess`, rather than hardcoding a warning.
 *
 * It used to return the same synced-folder warning for every vault, including
 * one at `/vaults`, so the tooltip claimed a path was a Google Drive folder
 * while the line directly above it said otherwise. That is worse than a
 * cosmetic bug: every screenshot taken to judge this UI was of a lie, and I
 * reviewed several of them without noticing.
 *
 * Name-based only, because that is the only signal a browser has. FUSE
 * detection needs a mount table and is deliberately not reported anyway.
 */
export function storageWarningFor(path: string): StorageWarning | null {
	const providers: [RegExp, string][] = [
		[/google[ _-]?drive/i, "Google Drive"],
		[/dropbox/i, "Dropbox"],
		[/onedrive/i, "OneDrive"],
		[/\bicloud/i, "iCloud Drive"],
	];

	for (const [pattern, provider] of providers) {
		if (!pattern.test(path)) continue;
		return {
			kind: "name",
			explanation: `A ${provider} folder can duplicate this file`,
			remedy: "Open it from Google Drive instead",
			providerHint: provider === "Google Drive" ? "google-drive" : null,
			why: `A sync daemon watching this folder can upload its own copy while Vardo is writing, which is how KeePass databases end up duplicated as "vault (1).kdbx". Vardo talking to ${provider} directly avoids the race entirely.`,
		};
	}
	return null;
}

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

export interface MockEntry {
	summary: EntrySummary;
	password: string;
	notes: string;
	totp: boolean;
	group: Uuid;
	/** Non-standard fields, so custom-attribute editing is exercised. */
	custom: Record<string, { value: string; protected: boolean }>;
	historyCount: number;
	/** Millis, or null for an entry that does not expire. */
	expires?: number | null;
	tags?: string[];
	/** Attached during this session, so the list reflects what was just done. */
	extraAttachments?: { name: string; size: number }[];
	removedAttachments?: string[];
}

/**
 * Nested on purpose, and more than two deep.
 *
 * A flat mock hid a real bug for weeks: the rail drew only the root's direct
 * children, so any nested group was invisible, and nothing on screen could
 * reveal it. Test data that is tidier than real data does not test anything.
 */
export const GROUPS: { uuid: Uuid; name: string; parent?: Uuid | undefined }[] = [
	{ uuid: "g-root", name: "passwords" },
	{ uuid: "g-email", name: "Email", parent: "g-root" },
	{ uuid: "g-finance", name: "Finance", parent: "g-root" },
	{ uuid: "g-banks", name: "Banks", parent: "g-finance" },
	{ uuid: "g-cards", name: "Cards", parent: "g-finance" },
	{ uuid: "g-infra", name: "Infrastructure", parent: "g-root" },
	{ uuid: "g-servers", name: "Servers", parent: "g-infra" },
	{ uuid: "g-eu", name: "eu-west", parent: "g-servers" },
	{ uuid: "g-shopping", name: "Shopping", parent: "g-root" },
	{ uuid: "g-gov", name: "Government & Admin", parent: "g-root" },
	{ uuid: "g-media", name: "Media", parent: "g-root" },
	{ uuid: "g-old", name: "Archive (pre-2015)", parent: "g-root" },
	// The last two are excluded from entry assignment, so both stay empty.
	{ uuid: "g-bin", name: "Recycle Bin", parent: "g-root" },
	{ uuid: "g-empty", name: "Work — new job", parent: "g-root" },
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
				iconPng: null,
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

/**
 * Scalable for testing. A vault of 412 hides every problem that only appears
 * at ten thousand, and "it feels fine here" is not a measurement.
 */
export const ENTRIES = makeEntries(
	Number(new URLSearchParams(location.search).get("n")) || 412,
);


/**
 * A fixed, deliberately awkward sync plan: one of everything the review
 * screen has to render, including the two conflict kinds that are easiest to
 * explain badly.
 */
export const DEMO_SYNC_PLAN: SyncPreview = {
	remote: "passwords.kdbx (local file)",
	remoteUnchanged: false,
	firstSync: false,
	// One of each shape, so the review screen is designed against a sync
	// that has something to say rather than the tidy case.
	warnings: [
		{
			kind: "clock-skew",
			detail:
				"The other copy is dated 3 hours in the future. Merges are decided by modification time, so a device with a wrong clock wins every conflict.",
		},
	],
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
