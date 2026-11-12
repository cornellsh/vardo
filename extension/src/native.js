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
 * Fills one login into the active tab's top frame.
 *
 * The URL sent to Vardo is the tab's, as Chrome reports it. Vardo checks the
 * login belongs to it before returning anything.
 *
 * @param {chrome.tabs.Tab} tab
 * @param {string} uuid
 * @returns {Promise<{ type: "filled", filled: string[], reason?: string } | Failure>}
 */
export async function fillInto(tab, uuid) {
	if (tab.id === undefined || !tab.url) return failure("internal", "No page to fill");
	const reply = await ask({ type: "fill", uuid, url: tab.url });
	if (reply.type !== "fill") return reply;

	try {
		const [result] = await chrome.scripting.executeScript({
			// The top frame only. Logins inside cross-origin iframes are a
			// classic way to attack autofill, and get their own design later.
			target: { tabId: tab.id, frameIds: [0] },
			func: fillPage,
			args: [
				new URL(tab.url).origin,
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
