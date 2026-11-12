// @ts-check

/**
 * Talking to Vardo, through the native host Chrome starts for this extension.
 *
 * Each call starts a host process, which hands the message to the running
 * Vardo over a local socket and exits when Chrome is done with it. Nothing is
 * kept between calls, so there is no connection to go stale while the popup is
 * closed or the service worker sleeps.
 */

import { fillPage } from "./fill.js";

/** The name the host was registered under by `vardo-browser-host --install`. */
export const HOST = "app.vardo.browser";
const VERSION = 1;

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
		return reply ?? failure("internal", "Vardo did not answer");
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
 * @param {string} code
 * @param {string} message
 * @returns {Failure}
 */
function failure(code, message) {
	return { type: "error", code, message };
}
