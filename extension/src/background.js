// @ts-check

/**
 * The keyboard shortcut.
 *
 * Pressing it is the user gesture that grants access to this tab, for this
 * page only. When there is exactly one login saved for exactly this site, it is
 * filled straight away. Anything else needs the person, so the popup opens:
 * several accounts, a login saved for a parent domain, a locked vault, or
 * Vardo not running.
 *
 * Subdomain matches are never filled from the shortcut. A script on any
 * subdomain could otherwise collect the main site's login, which is how ten of
 * eleven password managers were attacked in 2025.
 */

import { ask, fillInto } from "./native.js";

chrome.commands.onCommand.addListener(async (command, tab) => {
	if (command !== "fill-login" || !tab?.url || tab.id === undefined) return;

	const listed = await ask({ type: "logins", url: tab.url });
	if (listed.type === "logins") {
		const exact = listed.logins.filter((/** @type {{match?: string}} */ l) => l.match === "exact");
		if (exact.length === 1) {
			const result = await fillInto(tab, exact[0].uuid);
			if (result.type === "filled" && result.filled.length > 0) return;
		}
	}

	try {
		await chrome.action.openPopup();
	} catch {
		// Some Chromium builds refuse without a click on the toolbar button.
		// The toolbar button still works, so there is nothing more to do.
	}
});
