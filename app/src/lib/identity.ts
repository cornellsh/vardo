/**
 * How an entry presents itself: its colour, its initial, and its dates.
 *
 * Shared so the list and the detail panel cannot disagree. They did: the list
 * tinted avatars by UUID, so eight entries for the same site came out in eight
 * different colours and the list read as noise. Telegram gives one peer one
 * colour, and the equivalent of a peer here is the account you hold, not the
 * row that happens to hold it.
 */

/**
 * Telegram's contact palette. Seven, deliberately: enough that neighbours
 * rarely collide, few enough that a colour stays recognisable as "the Airbnb
 * one" rather than becoming decoration.
 */
const TINTS = [
	"#e17076",
	"#7bc862",
	"#65aadd",
	"#a695e7",
	"#ee7aae",
	"#6ec9cb",
	"#faa774",
] as const;

/**
 * The thing a person thinks of as "what this entry is for".
 *
 * The title first, and the host only when there is no title. Keying on the host
 * first looked more precise and was wrong: in a real vault some entries for a
 * site have the URL filled in and some do not, so the same site came out in two
 * colours and the list looked broken. What someone recognises is the name they
 * typed.
 */
export function identityOf(entry: {
	title: string;
	url?: string | null;
}): string {
	const title = entry.title.trim().toLowerCase();
	return title || hostOf(entry.url ?? "").toLowerCase();
}

export function hostOf(url: string): string {
	if (!url) return "";
	try {
		return new URL(
			url.includes("://") ? url : `https://${url}`,
		).hostname.replace(/^www\./, "");
	} catch {
		return "";
	}
}

/** Stable across runs and across machines, so a vault looks the same anywhere. */
export function tintFor(identity: string): string {
	let h = 0;
	for (const ch of identity) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
	return TINTS[h % TINTS.length] as string;
}

/**
 * One character, or two for a multi-word name.
 *
 * Uses `Intl.Segmenter` where available so an emoji or a Devanagari cluster
 * counts as one character. `title[0]` splits them mid-codepoint and renders a
 * broken glyph, which real vaults hit immediately.
 */
export function initialOf(title: string): string {
	const trimmed = title.trim();
	if (!trimmed) return "?";

	const words = trimmed.split(/\s+/).filter(Boolean);
	const first = firstGrapheme(words[0] ?? "");
	if (words.length === 1) return first.toUpperCase();

	// An emoji stands on its own. Pairing it with the next word's letter gave
	// "gift-box S" for an entry titled with an emoji and a name, which is two
	// glyphs of different sizes jammed into a forty-pixel circle. Only letters
	// pair with letters.
	if (!/\p{Letter}/u.test(first)) return first;

	const second = firstGrapheme(words[1] ?? "");
	if (!/\p{Letter}/u.test(second)) return first.toUpperCase();
	return (first + second).toUpperCase();
}

function firstGrapheme(text: string): string {
	if (!text) return "";
	if (typeof Intl !== "undefined" && "Segmenter" in Intl) {
		const seg = new Intl.Segmenter(undefined, { granularity: "grapheme" });
		for (const s of seg.segment(text)) return s.segment;
	}
	return Array.from(text)[0] ?? "";
}

/**
 * A compact age, for the right edge of a list row.
 *
 * Takes `now` so it can be tested. It read `Date.now()` directly and lived
 * inside a component, which is part of why the bug below survived: nothing
 * could examine it without rendering a list and looking at pixels.
 *
 * The weeks step is not decoration. Without it, anything between eight and
 * twenty-nine days old divided to zero months and rendered as "0mo".
 */
export function compactAge(ms: number, now: number = Date.now()): string {
	const days = Math.floor((now - ms) / 86_400_000);
	if (days <= 0) return "today";
	if (days === 1) return "1d";
	if (days < 7) return `${days}d`;
	if (days < 30) return `${Math.floor(days / 7)}w`;
	if (days < 365) return `${Math.max(1, Math.floor(days / 30))}mo`;
	return `${Math.max(1, Math.floor(days / 365))}y`;
}

/**
 * A date as a person would say it.
 *
 * `6/3/2026, 10:17:46 AM` is a machine's answer. Nobody needs the seconds a
 * password was edited, and "6/3" is ambiguous across exactly the two audiences
 * this app has. Recent things get a relative phrase because that is what the
 * question really is: did this change recently?
 */
export function humanDate(ms: number): string {
	if (!ms) return "never";
	const date = new Date(ms);
	const now = Date.now();
	const secs = Math.round((now - ms) / 1000);

	if (secs < 60) return "just now";
	if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
	if (secs < 86_400) return `${Math.floor(secs / 3600)} hr ago`;
	if (secs < 172_800) return "yesterday";
	if (secs < 604_800) return `${Math.floor(secs / 86_400)} days ago`;

	// Older than a week: an actual date, with the month spelled out so it reads
	// the same to everyone. The year only when it is not this one.
	const sameYear = date.getFullYear() === new Date().getFullYear();
	return date.toLocaleDateString(undefined, {
		day: "numeric",
		month: "short",
		...(sameYear ? {} : { year: "numeric" }),
	});
}
