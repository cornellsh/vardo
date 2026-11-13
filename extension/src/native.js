// @ts-check

/**
 * Talking to Vardo, through the native host Chrome starts for this extension.
 *
 * Each call starts a host process, which hands the message to the running
 * Vardo over a local socket and exits when Chrome is done with it. Nothing is
 * kept between calls, so there is no connection to go stale while the popup is
 * closed or the service worker sleeps.
 */

import { capturePage } from "./capture.js";
import { fillPage } from "./fill.js";

/** The name the host was registered under by `vardo-browser-host --install`. */
export const HOST = "app.vardo.browser";
const VERSION = 1;

/**
 * Every request the extension sends and every reply it understands, with the
 * fields each carries. `crates/vardo-browser/src/protocol.rs` is the truth.
 * Rust writes a golden file per message to `extension/protocol/`, and
 * `native.test.js` fails when this table and those files disagree.
 *
 * @type {Record<string, readonly string[]>}
 */
export const REQUESTS = {
	status: [],
	logins: ["url"],
	search: ["query"],
	fill: ["uuid", "url"],
	copy: ["uuid", "field"],
	show: [],
	launch: [],
	save: ["url", "username", "password", "replace"],
};

/** @type {Record<string, readonly string[]>} */
export const REPLIES = {
	status: ["state"],
	logins: ["logins", "truncated"],
	fill: ["username", "password", "totp"],
	copied: ["clear_after_seconds"],
	shown: [],
	launched: [],
	saved: ["uuid", "outcome"],
	error: ["code", "message"],
};

/** Vardo's error codes. The popup words some of them itself. */
export const ERROR_CODES = [
	"not-running",
	"locked",
	"not-a-match",
	"not-found",
	"version",
	"bad-request",
	"internal",
];
export const COPY_FIELDS = ["username", "password", "totp"];
export const STATE_KINDS = ["locked", "unlocked"];
/** What saving a login did. `differs` changed nothing and needs the person. */
export const SAVE_OUTCOMES = ["created", "updated", "unchanged", "differs"];

/**
 * Whether a reply is one this extension knows, with every field it expects.
 *
 * @param {unknown} reply
 * @returns {boolean}
 */
export function understood(reply) {
	if (!reply || typeof reply !== "object") return false;
	const message = /** @type {Record<string, unknown>} */ (reply);
	const expected = REPLIES[String(message.type)];
	return expected !== undefined && expected.every((field) => field in message);
}

let nextId = 1;

/**
 * @typedef {{ uuid: string, title: string, username: string, host: string | null,
 *             match?: "exact" | "subdomain", has_totp: boolean }} Login
 * @typedef {{ type: "error", code: string, message: string }} Failure
 */

/**
 * Sends one request and returns Vardo's reply, or a failure the popup can
 * explain. Never throws.
 *
 * @param {Record<string, unknown>} body
 * @returns {Promise<any>}
 */
export async function ask(body) {
	const message = { v: VERSION, id: nextId++, ...body };
	try {
		const reply = await chrome.runtime.sendNativeMessage(HOST, message);
		if (reply == null) return failure("internal", "Vardo did not answer");
		// A reply from a newer or older Vardo. The popup asks to update.
		if (!understood(reply)) return failure("version", "Vardo sent a reply this extension does not know");
		return reply;
	} catch (error) {
		const text = String(/** @type {Error} */ (error)?.message ?? error);
		// Chrome's wording when the host was never registered with this browser.
		if (/not found/i.test(text)) return failure("not-installed", text);
		return failure("host", text);
	}
}

/**
 * Which frame holds the form: the top frame when it has any login field,
 * otherwise the first frame offering a password field. Each probe is
 * `fillPage`'s dry run in that frame, so a hidden frame offers nothing.
 *
 * @param {{ frameId: number, url: string, filled: string[] }[]} probes
 * @returns {{ frameId: number, url: string } | null}
 */
export function chooseFrame(probes) {
	const web = probes.filter((p) => /^https?:/.test(p.url));
	const top = web.find((p) => p.frameId === 0);
	if (top && top.filled.length > 0) return top;
	const framed = web
		.filter((p) => p.frameId !== 0 && p.filled.includes("password"))
		.sort((a, b) => a.frameId - b.frameId)[0];
	return framed ?? null;
}

/**
 * Fills one login into the frame of the active tab that holds the form.
 *
 * The extension can only reach frames on the tab's own site (`activeTab`).
 * The URL sent to Vardo is the chosen frame's, so Vardo checks the login
 * belongs to that frame before returning anything, and the fill script checks
 * the frame's origin again before typing.
 *
 * @param {chrome.tabs.Tab} tab
 * @param {string} uuid
 * @returns {Promise<{ type: "filled", filled: string[], reason?: string } | Failure>}
 */
export async function fillInto(tab, uuid) {
	if (tab.id === undefined || !tab.url) return failure("internal", "No page to fill");
	let target = { frameId: 0, url: tab.url };
	try {
		const all = { tabId: tab.id, allFrames: true };
		const [urls, probes] = await Promise.all([
			chrome.scripting.executeScript({ target: all, func: () => location.href }),
			chrome.scripting.executeScript({ target: all, func: fillPage, args: [null, null] }),
		]);
		const urlOf = new Map(urls.map((u) => [u.frameId, String(u.result ?? "")]));
		target =
			chooseFrame(
				probes.map((p) => ({
					frameId: p.frameId,
					url: urlOf.get(p.frameId) ?? "",
					filled: p.result?.filled ?? [],
				})),
			) ?? target;
	} catch {
		// Probing failed, for example on a page that forbids scripts. The top
		// frame is still worth trying, and the fill reports what happened.
	}
	const reply = await ask({ type: "fill", uuid, url: target.url });
	if (reply.type !== "fill") return reply;

	try {
		const [result] = await chrome.scripting.executeScript({
			target: { tabId: tab.id, frameIds: [target.frameId] },
			func: fillPage,
			args: [
				new URL(target.url).origin,
				{ username: reply.username, password: reply.password, totp: reply.totp ?? null },
			],
		});
		const outcome = result?.result ?? { filled: [], reason: "no-result" };
		return { type: "filled", ...outcome };
	} catch (error) {
		return failure("page", String(/** @type {Error} */ (error)?.message ?? error));
	}
}

/**
 * Reads the login typed into the active tab's top frame.
 *
 * Called only from the popup's "Save this login" button, which is the gesture
 * that lets the extension see the page at all.
 *
 * @param {chrome.tabs.Tab} tab
 * @returns {Promise<{ type: "captured", url: string, username: string, password: string } | Failure>}
 */
export async function captureLogin(tab) {
	if (tab.id === undefined || !tab.url || !/^https?:/.test(tab.url)) {
		return failure("page", "Only logins on web pages can be saved");
	}
	try {
		const [result] = await chrome.scripting.executeScript({
			target: { tabId: tab.id, frameIds: [0] },
			func: capturePage,
		});
		const found = result?.result;
		if (!found || "reason" in found) return failure("page", "Type the password into the page first");
		return { type: "captured", ...found };
	} catch (error) {
		return failure("page", String(/** @type {Error} */ (error)?.message ?? error));
	}
}

/**
 * @param {string} code
 * @param {string} message
 * @returns {Failure}
 */
function failure(code, message) {
	return { type: "error", code, message };
}
