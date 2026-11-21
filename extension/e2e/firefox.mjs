// Drives headless Firefox over WebDriver BiDi for run.mjs.
//
// BiDi cannot click a toolbar button or reach an extension popup, so this asks
// Firefox itself, from its privileged browser window (MOZ_REMOTE_ALLOW_SYSTEM_ACCESS):
//   - the click is Firefox's own toolbar-button handler, which also grants the
//     extension the tab the way a person's click does;
//   - the popup is read and pressed through the accessibility tree, the same
//     names and buttons a screen reader gets.
// Both use Firefox internals, so a Firefox release can break this file without
// the extension being wrong. Needs Firefox 138 or later (set FIREFOX).

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const GECKO_ID = "browser@vardo.app";
const BIDI_PORT = 19444;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Where `vardo-browser-host --install` looks for Firefox under `home`. */
export function profileDir({ home, mac }) {
	return mac ? join(home, "Library", "Application Support", "Firefox") : join(home, ".mozilla", "firefox");
}

// Runs in the browser window. Defines the helpers the popup calls go through.
const HELPERS = `(() => {
	const svc = Cc["@mozilla.org/accessibilityService;1"].getService(Ci.nsIAccessibilityService);
	const R = Ci.nsIAccessibleRole;
	const browser = () => [...document.querySelectorAll("browser")].filter((b) => b.currentURI?.spec.endsWith("/src/popup.html")).at(-1);
	function* walk(a) { yield a; for (let c = a.firstChild; c; c = c.nextSibling) yield* walk(c); }
	const nodes = () => { const b = browser(); const doc = b && svc.getAccessibleFor(b); return doc ? [...walk(doc)] : []; };
	const act = (a) => { if (!a) return false; a.doAction(0); return true; };
	window.vardoE2E = {
		id: () => browser()?.browsingContext?.id ?? null,
		// The toolbar button toggles, so a popup still open would just close.
		close: () => { browser()?.closest("panel")?.hidePopup(); return true; },
		text: () => nodes().filter((a) => a.role === R.ROLE_TEXT_LEAF).map((a) => a.name).join("\\n"),
		choose: () => act(nodes().find((a) => a.role === R.ROLE_OPTION)),
		press: (label) => act(nodes().find((a) => a.role === R.ROLE_PUSHBUTTON && (a.name ?? "").startsWith(label))),
		trigger: () => {
			const { ExtensionParent } = ChromeUtils.importESModule("resource://gre/modules/ExtensionParent.sys.mjs");
			const ext = ExtensionParent.GlobalManager.getExtension(${JSON.stringify(GECKO_ID)});
			ext.apiManager.global.browserActionFor(ext).triggerAction(window);
		},
	};
	return true;
})()`;

/**
 * Starts Firefox with the Firefox build of the extension installed.
 * `track(child)` hands its process group to run.mjs to stop.
 */
export async function start({ extension, work, home, env, mac, track }) {
	const built = spawnSync(process.execPath, ["scripts/firefox.mjs"], { cwd: extension, encoding: "utf8" });
	if (built.status !== 0) throw new Error(`build:firefox failed: ${built.stderr}`);
	const profile = join(work, "firefox-profile");
	mkdirSync(profile, { recursive: true });
	const browser = spawn(
		process.env.FIREFOX ?? "firefox",
		["--headless", "--no-remote", "--profile", profile, "--remote-debugging-port", String(BIDI_PORT), "about:blank"],
		// HOME, because on Linux that is where Firefox looks for native hosts.
		{ env: { ...env, HOME: home, MOZ_REMOTE_ALLOW_SYSTEM_ACCESS: "1" }, stdio: "ignore", detached: true },
	);
	track(browser);

	let ws;
	for (let i = 0; i < 60 && !ws; i++) {
		await sleep(250);
		ws = await new Promise((resolve) => {
			const s = new WebSocket(`ws://127.0.0.1:${BIDI_PORT}/session`);
			s.addEventListener("open", () => resolve(s));
			s.addEventListener("error", () => resolve(undefined));
		});
	}
	if (!ws) throw new Error("Firefox did not start. Set FIREFOX to its path.");
	let nextId = 1;
	const pending = new Map();
	ws.addEventListener("message", (e) => {
		let m;
		try {
			m = JSON.parse(e.data);
		} catch {
			return; // Not a reply to anything we asked.
		}
		if (m.id && pending.has(m.id)) {
			pending.get(m.id)(m);
			pending.delete(m.id);
		}
	});
	const send = async (method, params = {}) => {
		const id = nextId++;
		ws.send(JSON.stringify({ id, method, params }));
		const m = await new Promise((r) => pending.set(id, r));
		if (m.type === "error") throw new Error(`${method}: ${m.message}`);
		return m.result;
	};
	/** Evaluates in `context` and returns the value as JSON would carry it. */
	const evaluate = async (context, expression) => {
		const r = await send("script.evaluate", {
			expression: `JSON.stringify(eval(${JSON.stringify(expression)}) ?? null)`,
			target: { context },
			awaitPromise: true,
		});
		if (r.type === "exception") throw new Error(r.exceptionDetails.text);
		return JSON.parse(r.result.value);
	};

	await send("session.new", { capabilities: {} });
	const chrome = (await send("browsingContext.getTree", { "moz:scope": "chrome" })).contexts[0].context;
	await evaluate(chrome, HELPERS);
	if (mac) {
		// On macOS Firefox finds native hosts under the real home folder whatever
		// HOME says. Point it at the test's own, before anything looks one up.
		const dir = join(home, "Library", "Application Support", "Mozilla");
		await evaluate(
			chrome,
			`(() => {
				const f = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
				f.initWithPath(${JSON.stringify(dir)});
				try { Services.dirsvc.undefine("XREUserNativeManifests"); } catch {}
				Services.dirsvc.set("XREUserNativeManifests", f);
				// It reads the folder once, possibly already at startup.
				ChromeUtils.importESModule("resource://gre/modules/NativeManifests.sys.mjs").NativeManifests._initializePromise = null;
				return true;
			})()`,
		);
	}
	await send("webExtension.install", { extensionData: { type: "path", path: join(extension, "dist", "firefox") } });

	let lastPopup = null;

	/** Opens `url` in a tab and clicks the toolbar button. */
	async function openPopup(url) {
		const { context: tab } = await send("browsingContext.create", { type: "tab" });
		await send("browsingContext.navigate", { context: tab, url, wait: "complete" });
		await send("browsingContext.activate", { context: tab });
		await evaluate(chrome, "vardoE2E.close()");
		await evaluate(chrome, "vardoE2E.trigger()");
		let id = null;
		for (let i = 0; i < 40 && (id === null || id === lastPopup); i++) {
			await sleep(250);
			id = await evaluate(chrome, "vardoE2E.id()");
		}
		if (id === null || id === lastPopup) throw new Error("the popup never opened");
		lastPopup = id;
		const popup = {
			text: () => evaluate(chrome, "vardoE2E.text()"),
			choose: () => evaluate(chrome, "vardoE2E.choose()"),
			press: (label) => evaluate(chrome, `vardoE2E.press(${JSON.stringify(label)})`),
		};
		return { popup, page: (expression) => evaluate(tab, expression) };
	}

	return { openPopup };
}
