// @ts-check

/**
 * The popup: choose a login for this page, or find one in the whole vault.
 *
 * It lives in the browser's own UI, not in the page, so a page cannot cover
 * it, hide it or click it on the person's behalf.
 */

import { hostOf, identityOf, initialOf, tintFor } from "./identity.js";
import { ask, captureLogin, fillInto } from "./native.js";

/** @typedef {import("./native.js").Login} Login */

const content = /** @type {HTMLElement} */ (document.getElementById("content"));
const search = /** @type {HTMLInputElement} */ (document.getElementById("search"));
const toast = /** @type {HTMLElement} */ (document.getElementById("toast"));

/** @type {chrome.tabs.Tab | undefined} */
let tab;
/** @type {HTMLElement[]} */
let rows = [];
let selected = -1;

start();

async function start() {
	[tab] = await chrome.tabs.query({ active: true, currentWindow: true });
	await refresh();
}

/** Asks Vardo where things stand, and draws the matching screen. */
async function refresh() {
	const status = await ask({ type: "status" });

	if (status.type === "error") return showProblem(status);
	if (status.state.kind === "locked") return showLocked();

	search.hidden = false;
	search.focus();
	await showMatches();
	await showShortcut();
	showSaveLink();
}

/**
 * Says which key fills from any page, or that there is none.
 *
 * A browser silently refuses a suggested shortcut it already uses itself.
 * Chrome did exactly that to the first choice, Ctrl+Shift+L, in 2026, and the
 * only symptom was a key that did nothing. So the popup checks.
 */
async function showShortcut() {
	const commands = await chrome.commands.getAll();
	const shortcut = commands.find((c) => c.name === "fill-login")?.shortcut;
	const note = document.createElement("p");
	note.className = "shortcut";
	if (shortcut) {
		const key = document.createElement("kbd");
		key.textContent = shortcut;
		note.append(key, " fills from any page");
	} else {
		const link = document.createElement("button");
		link.type = "button";
		link.className = "link";
		link.textContent = "Set a key to fill from any page";
		link.addEventListener("click", () => {
			void chrome.tabs.create({ url: "chrome://extensions/shortcuts" });
			window.close();
		});
		note.append(link);
	}
	document.body.append(note);
}

/**
 * Offers to keep the login typed into this page.
 *
 * Nothing reads the page until this is clicked
 * (`inv:extension-sees-nothing-until-asked`), so there is no prompt after a
 * sign-in. Asking is the person's move.
 */
function showSaveLink() {
	if (!tab?.url || !/^https?:/.test(tab.url)) return;
	const note = document.createElement("p");
	note.className = "shortcut";
	const link = document.createElement("button");
	link.type = "button";
	link.className = "link";
	link.textContent = "Save the login typed on this page";
	link.addEventListener("click", () => void saveLogin());
	note.append(link);
	document.body.append(note);
}

// --- screens --------------------------------------------------------------------

/** @param {{ code: string, message: string }} problem */
function showProblem(problem) {
	search.hidden = true;
	if (problem.code === "not-installed") {
		return state(
			"Connect Vardo to this browser",
			"Run this once in a terminal, then open this again:",
			{ code: "vardo-browser-host --install" },
		);
	}
	if (problem.code === "not-running") {
		return state("Vardo isn't running", "Start it to fill passwords.", {
			label: "Start Vardo",
			action: launch,
		});
	}
	if (problem.code === "version") {
		return state("Update Vardo", "The extension and the app are different versions.");
	}
	return state("Vardo didn't answer", problem.message);
}

function showLocked() {
	search.hidden = true;
	state("Vardo is locked", "Unlock it, then open this again.", {
		label: "Unlock in Vardo",
		action: async () => {
			await ask({ type: "show" });
			window.close();
		},
	});
}

async function showMatches() {
	if (!tab?.url || !/^https?:/.test(tab.url)) {
		return list([], "Search for a login to copy it.");
	}
	const reply = await ask({ type: "logins", url: tab.url });
	if (reply.type === "error") return showProblem(reply);

	/** @type {Login[]} */
	const logins = reply.logins;
	const exact = logins.filter((l) => l.match === "exact");
	const nearby = logins.filter((l) => l.match === "subdomain");
	const host = hostOf(tab.url) || "this site";

	content.replaceChildren();
	rows = [];
	if (exact.length === 0 && nearby.length === 0) {
		content.append(empty(`No logins saved for ${host}`));
	}
	if (exact.length) {
		content.append(section(`For ${host}`), ...exact.map((l) => row(l, true)));
	}
	if (nearby.length) {
		// Never filled from the shortcut. Shown here, labelled, so choosing one
		// is a decision rather than something that just happens.
		content.append(section("Saved for a parent site"), ...nearby.map((l) => row(l, true)));
	}
	if (reply.truncated) content.append(empty("More logins match. Search to narrow them down."));
	select(exact.length || nearby.length ? 0 : -1);
}

/** @param {string} query */
async function showSearch(query) {
	const reply = await ask({ type: "search", query });
	if (reply.type === "error") return showProblem(reply);
	// Search results that do not match the page can be copied, never filled:
	// Vardo would refuse, and the refusal is right.
	list(reply.logins, `Nothing matches "${query}"`, reply.truncated);
}

/**
 * @param {Login[]} logins
 * @param {string} emptyText
 * @param {boolean} [truncated]
 */
function list(logins, emptyText, truncated = false) {
	content.replaceChildren();
	rows = [];
	if (logins.length === 0) content.append(empty(emptyText));
	content.append(...logins.map((l) => row(l, Boolean(l.match))));
	if (truncated) content.append(empty("More logins match. Keep typing to narrow them down."));
	select(logins.length ? 0 : -1);
}

// --- pieces ---------------------------------------------------------------------

/**
 * @param {Login} login
 * @param {boolean} fills Whether choosing the row fills the page.
 */
function row(login, fills) {
	const el = document.createElement("div");
	el.className = fills ? "row fills" : "row";
	el.tabIndex = -1;
	el.setAttribute("role", "option");

	const avatar = document.createElement("span");
	avatar.className = "avatar";
	avatar.style.background = tintFor(identityOf({ title: login.title, url: login.host }));
	avatar.textContent = initialOf(login.title || login.host || "?");
	avatar.setAttribute("aria-hidden", "true");

	const text = document.createElement("div");
	text.className = "text";
	const title = document.createElement("div");
	title.className = "title";
	title.textContent = login.title || login.host || "Untitled";
	const sub = document.createElement("div");
	sub.className = "sub";
	sub.textContent = [login.username, fills ? null : login.host].filter(Boolean).join(" at ") || "No username";
	text.append(title, sub);

	const actions = document.createElement("div");
	actions.className = "actions";
	if (login.username) actions.append(iconButton("Copy username", icon(USER_ICON), () => copy(login, "username")));
	actions.append(iconButton("Copy password", icon(KEY_ICON), () => copy(login, "password")));
	if (login.has_totp) actions.append(iconButton("Copy code", icon(CLOCK_ICON), () => copy(login, "totp")));

	el.append(avatar, text, actions);
	if (fills) {
		el.title = "Fill";
		el.addEventListener("click", (e) => {
			if (/** @type {HTMLElement} */ (e.target).closest(".icon")) return;
			void fill(login);
		});
	}
	el.addEventListener("mouseenter", () => select(rows.indexOf(el)));
	el.dataset.uuid = login.uuid;
	el.dataset.fills = String(fills);
	rows.push(el);
	return el;
}

/**
 * @param {string} label
 * @param {SVGElement} svg
 * @param {() => void} onClick
 */
function iconButton(label, svg, onClick) {
	const button = document.createElement("button");
	button.className = "icon";
	button.type = "button";
	button.title = label;
	button.setAttribute("aria-label", label);
	button.append(svg);
	button.addEventListener("click", (e) => {
		e.stopPropagation();
		onClick();
	});
	return button;
}

/** @param {string} text */
function section(text) {
	const el = document.createElement("div");
	el.className = "section";
	el.textContent = text;
	return el;
}

/** @param {string} text */
function empty(text) {
	const el = document.createElement("div");
	el.className = "empty";
	el.textContent = text;
	return el;
}

/**
 * @param {string} heading
 * @param {string} body
 * @param {{ label?: string, action?: () => unknown, code?: string }} [extra]
 */
function state(heading, body, extra = {}) {
	const el = document.createElement("div");
	el.className = "state";
	const img = document.createElement("img");
	img.src = "../icons/icon-48.png";
	img.alt = "";
	const h = document.createElement("h1");
	h.textContent = heading;
	const p = document.createElement("p");
	p.textContent = body;
	el.append(img, h, p);
	if (extra.code) {
		const code = document.createElement("code");
		code.textContent = extra.code;
		el.append(code);
	}
	if (extra.label && extra.action) {
		const button = document.createElement("button");
		button.className = "button";
		button.textContent = extra.label;
		const action = extra.action;
		button.addEventListener("click", async () => {
			button.disabled = true;
			await action();
			button.disabled = false;
		});
		el.append(button);
		queueMicrotask(() => button.focus());
	}
	content.replaceChildren(el);
	rows = [];
}

// --- actions --------------------------------------------------------------------

/** @param {Login} login */
async function fill(login) {
	if (!tab) return;
	const result = await fillInto(tab, login.uuid);
	if (result.type === "error") return say(result.message, true);
	if (result.filled.length === 0) {
		return say(
			result.reason === "new-password"
				? "This looks like a sign-up form"
				: "No login fields on this page",
			true,
		);
	}
	window.close();
}

/**
 * Vardo copies, and clears the clipboard afterwards. The password never comes
 * to the browser at all.
 *
 * @param {Login} login
 * @param {"username" | "password" | "totp"} field
 */
async function copy(login, field) {
	const reply = await ask({ type: "copy", uuid: login.uuid, field });
	if (reply.type === "error") return say(reply.message, true);
	const what = { username: "Username", password: "Password", totp: "Code" }[field];
	say(`${what} copied. Clears in ${reply.clear_after_seconds}s`);
}

async function saveLogin() {
	if (!tab) return;
	const captured = await captureLogin(tab);
	if (captured.type === "error") return say(captured.message, true);
	const login = { url: captured.url, username: captured.username, password: captured.password };
	const reply = await ask({ type: "save", ...login, replace: false });
	if (reply.type === "error") return say(reply.message, true);
	if (reply.outcome !== "differs") return say(SAVED[reply.outcome] ?? "Saved");

	const who = login.username || "this login";
	search.hidden = true;
	state(
		"Replace the saved password?",
		`Vardo has a different password for ${who} on ${hostOf(login.url) || "this site"}. The old one stays in its history.`,
		{
			label: "Replace",
			action: async () => {
				const again = await ask({ type: "save", ...login, replace: true });
				if (again.type === "error") return say(again.message, true);
				search.hidden = false;
				await showMatches();
				say(SAVED[again.outcome] ?? "Saved");
			},
		},
	);
}

/** @type {Record<string, string>} */
const SAVED = {
	created: "Saved to Vardo",
	updated: "Password updated in Vardo",
	unchanged: "Vardo already has this login",
};

async function launch() {
	await ask({ type: "launch" });
	// Vardo takes a moment to start and open its socket.
	for (let i = 0; i < 20; i++) {
		await new Promise((r) => setTimeout(r, 500));
		const status = await ask({ type: "status" });
		if (status.type === "status") return refresh();
	}
	say("Vardo didn't start", true);
}

/**
 * @param {string} text
 * @param {boolean} [bad]
 */
function say(text, bad = false) {
	toast.textContent = text;
	toast.className = bad ? "bad" : "";
	toast.hidden = false;
}

// --- keyboard -------------------------------------------------------------------

/** @param {number} index */
function select(index) {
	selected = index;
	rows.forEach((r, i) => {
		r.classList.toggle("selected", i === index);
		r.setAttribute("aria-selected", String(i === index));
	});
	rows[index]?.scrollIntoView({ block: "nearest" });
}

let searchTimer = 0;
search.addEventListener("input", () => {
	clearTimeout(searchTimer);
	const query = search.value.trim();
	searchTimer = setTimeout(() => void (query ? showSearch(query) : showMatches()), 120);
});

document.addEventListener("keydown", (e) => {
	if (e.key === "ArrowDown" && rows.length) {
		e.preventDefault();
		select(Math.min(selected + 1, rows.length - 1));
	} else if (e.key === "ArrowUp" && rows.length) {
		e.preventDefault();
		select(Math.max(selected - 1, 0));
	} else if (e.key === "Enter" && rows[selected]?.dataset.fills === "true") {
		e.preventDefault();
		rows[selected].click();
	}
});

// --- icons ----------------------------------------------------------------------

// Built as elements rather than parsed from markup, so nothing in the popup
// ever goes through an HTML parser.
/** @typedef {{ circle: [number, number, number], path: string }} IconShape */

/** @type {IconShape} */
const USER_ICON = { circle: [12, 8, 4], path: "M4 21c0-4 4-6 8-6s8 2 8 6" };
/** @type {IconShape} */
const KEY_ICON = { circle: [8, 15, 4], path: "M11 12l9-9M17 6l3 3M14 9l2 2" };
/** @type {IconShape} */
const CLOCK_ICON = { circle: [12, 12, 9], path: "M12 7v5l3 2" };

/** @param {IconShape} shape */
function icon(shape) {
	const NS = "http://www.w3.org/2000/svg";
	const svg = document.createElementNS(NS, "svg");
	for (const [k, v] of Object.entries({
		viewBox: "0 0 24 24",
		fill: "none",
		stroke: "currentColor",
		"stroke-width": "2",
		"stroke-linecap": "round",
		"stroke-linejoin": "round",
		"aria-hidden": "true",
	})) {
		svg.setAttribute(k, v);
	}
	const circle = document.createElementNS(NS, "circle");
	const [cx, cy, r] = shape.circle;
	circle.setAttribute("cx", String(cx));
	circle.setAttribute("cy", String(cy));
	circle.setAttribute("r", String(r));
	const path = document.createElementNS(NS, "path");
	path.setAttribute("d", shape.path);
	svg.append(circle, path);
	return svg;
}
