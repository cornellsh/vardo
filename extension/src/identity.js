// @ts-check

/**
 * A login's colour and initial, so it looks the same in the popup as in Vardo.
 *
 * A copy of `app/src/lib/identity.ts`, because the extension has no build step
 * to share it with. `identity.test.js` runs both on the same inputs and fails
 * if they disagree, so the copy cannot drift without someone noticing.
 */

const TINTS = ["#e17076", "#7bc862", "#65aadd", "#a695e7", "#ee7aae", "#6ec9cb", "#faa774"];

/** @param {{ title: string, url?: string | null }} entry */
export function identityOf(entry) {
	const title = entry.title.trim().toLowerCase();
	return title || hostOf(entry.url ?? "").toLowerCase();
}

/** @param {string} url */
export function hostOf(url) {
	if (!url) return "";
	try {
		return new URL(url.includes("://") ? url : `https://${url}`).hostname.replace(/^www\./, "");
	} catch {
		return "";
	}
}

/** @param {string} identity */
export function tintFor(identity) {
	let h = 0;
	for (const ch of identity) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
	return TINTS[h % TINTS.length];
}

/** @param {string} title */
export function initialOf(title) {
	const trimmed = title.trim();
	if (!trimmed) return "?";
	const words = trimmed.split(/\s+/).filter(Boolean);
	const first = firstGrapheme(words[0] ?? "");
	if (words.length === 1) return first.toUpperCase();
	if (!/\p{Letter}/u.test(first)) return first;
	const second = firstGrapheme(words[1] ?? "");
	if (!/\p{Letter}/u.test(second)) return first.toUpperCase();
	return (first + second).toUpperCase();
}

/** @param {string} text */
function firstGrapheme(text) {
	if (!text) return "";
	const seg = new Intl.Segmenter(undefined, { granularity: "grapheme" });
	for (const s of seg.segment(text)) return s.segment;
	return "";
}
