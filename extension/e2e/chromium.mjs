// Drives headless Chromium over the DevTools protocol for run.mjs.
//
// Needs Chromium itself: branded Chrome ignores --load-extension since 137.

import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const EXT_ID = "fkemoafedkbgljikdhjbglhlhfplamll";
const DEBUG_PORT = 19333;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Where `vardo-browser-host --install` looks for Chromium under `home`. */
export function profileDir({ home, env, mac }) {
	return mac ? join(home, "Library", "Application Support", "Chromium") : join(env.XDG_CONFIG_HOME, "chromium");
}

/**
 * Starts Chromium with the extension loaded. `track(child)` hands its process
 * group to run.mjs to stop.
 */
export async function start({ extension, home, env, mac, track }) {
	const profile = profileDir({ home, env, mac });
	mkdirSync(profile, { recursive: true });
	const browser = spawn(
		process.env.CHROMIUM ?? "chromium",
		[
			"--headless=new",
			// Ubuntu 24.04 runners forbid the unprivileged namespaces Chromium's
			// sandbox needs. The pages here are local test fixtures; nowhere else.
			...(process.env.CI ? ["--no-sandbox"] : []),
			// Otherwise macOS asks for the login keychain, which a headless run
			// cannot answer.
			...(mac ? ["--use-mock-keychain"] : []),
			"--enable-unsafe-extension-debugging",
			`--user-data-dir=${profile}`,
			`--load-extension=${extension}`,
			`--disable-extensions-except=${extension}`,
			`--remote-debugging-port=${DEBUG_PORT}`,
			"--no-first-run",
			"--no-default-browser-check",
			"--disable-features=DisableLoadExtensionCommandLineSwitch",
			"about:blank",
		],
		// Its own process group, so run.mjs can stop its helpers too.
		{ env, stdio: "ignore", detached: true },
	);
	track(browser);

	let version;
	for (let i = 0; i < 60 && !version; i++) {
		await sleep(250);
		version = await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/version`).then((r) => r.json()).catch(() => null);
	}
	if (!version) throw new Error("Chromium did not start. Set CHROMIUM to its path.");

	const ws = new WebSocket(version.webSocketDebuggerUrl);
	await new Promise((r) => ws.addEventListener("open", r));
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
	const send = (method, params = {}, sessionId) => {
		const id = nextId++;
		ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
		return new Promise((r) => pending.set(id, r));
	};
	const evaluate = async (sessionId, expression) => {
		const r = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
		return r.result?.result?.value;
	};
	const targets = async (type) =>
		(await send("Target.getTargets", type ? { filter: [{ type }] } : {})).result.targetInfos;
	const attach = async (targetId) =>
		(await send("Target.attachToTarget", { targetId, flatten: true })).result.sessionId;

	/** Opens `url` in a tab and clicks the toolbar button. */
	async function openPopup(url) {
		await send("Target.createTarget", { url });
		// Polled rather than slept for: a fixed wait was long enough on an idle
		// machine and not on a busy one, and one run in five failed that way.
		let tab;
		let page;
		for (let i = 0; i < 40 && !(tab && page); i++) {
			await sleep(200);
			tab = (await targets("tab")).find((t) => t.url === url);
			page = (await targets()).find((t) => t.type === "page" && t.url === url);
		}
		if (!tab || !page) throw new Error(`could not find the tab for ${url}`);
		const pageSession = await attach(page.targetId);
		for (let i = 0; i < 40; i++) {
			if ((await evaluate(pageSession, "document.readyState")) === "complete") break;
			await sleep(100);
		}
		await send("Target.activateTarget", { targetId: page.targetId });
		const triggered = await send("Extensions.triggerAction", { id: EXT_ID, targetId: tab.targetId });
		if (triggered.error) throw new Error(`triggerAction: ${triggered.error.message}`);
		let popup;
		for (let i = 0; i < 40 && !popup; i++) {
			await sleep(250);
			popup = (await targets()).find((t) => t.url.startsWith(`chrome-extension://${EXT_ID}/src/popup.html`));
		}
		if (!popup) throw new Error("the popup never opened");
		const session = await attach(popup.targetId);
		const click = (finder) => evaluate(session, `(${finder})()?.click()`);
		return {
			popup: {
				text: () => evaluate(session, "document.body.innerText"),
				choose: () => click(`() => document.querySelector(".row.fills")`),
				press: (label) =>
					click(
						`() => [...document.querySelectorAll("button")].find((b) => (b.getAttribute("aria-label") ?? b.textContent).trim().startsWith(${JSON.stringify(label)}))`,
					),
			},
			page: (expression) => evaluate(pageSession, expression),
		};
	}

	return { openPopup };
}
