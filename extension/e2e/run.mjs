// The extension, end to end, in a real Chromium.
//
//   npm run e2e            (from extension/)
//
// Starts, in a private temporary directory:
//   - a small web server with a sign-in page and a two-factor page,
//   - Vardo's browser socket for a vault built by Vardo's own code
//     (`browser_e2e_server`, the exact server the app runs, without a window),
//   - headless Chromium with this extension loaded and the native host
//     registered with it by `vardo-browser-host --install`.
//
// Then it clicks the toolbar button the way a person would, which is what
// grants the extension access to the tab, and checks what the popup shows and
// what lands in the page. Nothing touches the real clipboard, the real browser
// profile or a Vardo you are running.
//
// Pressing the keyboard shortcut cannot be simulated: key events sent over the
// DevTools protocol reach the page, not the browser. The run checks instead
// that the shortcut was assigned, which is how it failed in practice.
//
// Needs: cargo, and Chromium (set CHROMIUM if it is not `chromium`).

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const EXT_ID = "fkemoafedkbgljikdhjbglhlhfplamll";
const here = dirname(fileURLToPath(import.meta.url));
const extension = resolve(here, "..");
const repo = resolve(extension, "..");
const chromium = process.env.CHROMIUM ?? "chromium";
const SITE_PORT = 18765;
const DEBUG_PORT = 19333;

const work = mkdtempSync(join(tmpdir(), "vardo-e2e-"));
const env = {
	...process.env,
	XDG_RUNTIME_DIR: join(work, "run"),
	XDG_CONFIG_HOME: join(work, "config"),
};
mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
const profile = join(env.XDG_CONFIG_HOME, "chromium");
mkdirSync(profile, { recursive: true });

const children = [];
let failures = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function check(name, ok, detail = "") {
	console.log(`${ok ? "  ok  " : "  FAIL"} ${name}${ok ? "" : `\n         ${detail}`}`);
	if (!ok) failures++;
}

/**
 * Stops everything, waits for it to stop, then deletes the directory.
 *
 * Chromium is not one process. Signalling only the one it started with left
 * its renderer and GPU helpers writing the profile while it was deleted, so
 * the delete failed after every check had passed. It runs in its own process
 * group, the whole group is signalled, and the delete retries briefly. A
 * cleanup problem is reported but never turns a passing run into a failure.
 */
async function finish(code) {
	const exited = children
		.filter((child) => child.exitCode === null && child.signalCode === null)
		.map((child) => new Promise((r) => child.once("exit", r)));
	for (const child of children) {
		try {
			// A negative pid signals the group, for children started detached.
			process.kill(child.spawnargs.includes("--headless=new") ? -child.pid : child.pid, "SIGTERM");
		} catch {
			// Already gone.
		}
	}
	await Promise.race([Promise.all(exited), sleep(5000)]);
	try {
		rmSync(work, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
	} catch (error) {
		console.warn(`could not remove ${work}: ${error.message}`);
	}
	process.exit(code);
}
process.on("SIGINT", () => void finish(130));

// --- build ------------------------------------------------------------------------

console.log("building the host and the test server");
const build = spawnSync(
	"cargo",
	["build", "--quiet", "-p", "vardo-browser", "-p", "vardo-core", "--example", "browser_e2e_server", "--example", "browser_e2e_vault", "--bin", "vardo-browser-host"],
	{ cwd: repo, stdio: "inherit" },
);
if (build.status !== 0) await finish(1);
const target = join(repo, "target", "debug");
const host = join(target, "vardo-browser-host");

// --- the vault, the socket, the site ----------------------------------------------

const vault = join(work, "e2e.kdbx");
spawnSync(join(target, "examples", "browser_e2e_vault"), [vault, String(SITE_PORT)], { stdio: "ignore" });

const server = spawn(join(target, "examples", "browser_e2e_server"), [vault, "demopass"], { env });
children.push(server);
let serverLog = "";
server.stdout.on("data", (d) => (serverLog += d));

const site = createServer((req, res) => {
	try {
		const name = new URL(req.url, "http://x").pathname.replace(/^\//, "") || "login.html";
		// Read before writing a header, so a missing file (the browser always
		// asks for favicon.ico) can still become a 404.
		const body = readFileSync(join(here, "site", name.replace(/[^a-z0-9.]/g, "")));
		res.writeHead(200, { "content-type": "text/html" });
		res.end(body);
	} catch {
		res.writeHead(404);
		res.end();
	}
}).listen(SITE_PORT, "127.0.0.1");

const registered = spawnSync(host, ["--install"], { env, encoding: "utf8" });
check("the host registers itself with the browser", registered.status === 0, registered.stderr);

// --- the browser --------------------------------------------------------------------

const browser = spawn(
	chromium,
	[
		"--headless=new",
		// Ubuntu 24.04 runners forbid the unprivileged namespaces Chromium's
		// sandbox needs. The pages here are local test fixtures; nowhere else.
		...(process.env.CI ? ["--no-sandbox"] : []),
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
	// Its own process group, so `finish` can stop its helpers too.
	{ env, stdio: "ignore", detached: true },
);
children.push(browser);

let version;
for (let i = 0; i < 60 && !version; i++) {
	await sleep(250);
	version = await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/version`).then((r) => r.json()).catch(() => null);
}
if (!version) {
	console.error("Chromium did not start. Set CHROMIUM to its path.");
	await finish(1);
}

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

let opened = 0;

/** Opens `path` in a tab, clicks the toolbar button, returns the popup's session. */
async function openPopup(path) {
	// A unique address each time, so the tab and the page found below are
	// certainly the ones just opened. The first version matched on the path,
	// found an older tab, and read back a page that was never filled.
	const url = `http://127.0.0.1:${SITE_PORT}${path}?run=${++opened}`;
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
	// Wait for it to hear back from Vardo.
	for (let i = 0; i < 40; i++) {
		await sleep(250);
		const text = await evaluate(session, "document.body.innerText");
		if (text && !/^\s*$/.test(text)) break;
	}
	await sleep(300);
	return { session, page: pageSession };
}

const fields = (session) =>
	evaluate(session, `Object.fromEntries([...document.querySelectorAll('input')].map(i => [i.id, i.value]))`);

// --- the checks -----------------------------------------------------------------------

console.log("signing in");
{
	const { session, page } = await openPopup("/login.html");
	const text = await evaluate(session, "document.body.innerText");
	check("the popup lists the login saved for this site", text.includes("Test site"), text);
	check("a login saved for another port is not offered", !text.includes("Other service"), text);
	check("the keyboard shortcut was assigned", /Alt\+Shift\+L/.test(text), text);

	await evaluate(session, "document.querySelector('.row.fills').click()");
	await sleep(1200);
	const values = await fields(page);
	check("the username is filled", values.email === "e2e-user@example.com", JSON.stringify(values));
	check("the password is filled", values.password === "e2e-password-123", JSON.stringify(values));
	check("the invisible trap form is left empty", values["trap-user"] === "" && values["trap-pass"] === "", JSON.stringify(values));
	check("the site's search box is left alone", values.search === "", JSON.stringify(values));
}

console.log("copying");
{
	const { session } = await openPopup("/login.html");
	await evaluate(session, `document.querySelector('button[aria-label="Copy password"]').click()`);
	await sleep(800);
	const text = await evaluate(session, "document.body.innerText");
	check("the popup says the password was copied", text.includes("Password copied"), text);
	check("Vardo did the copying, not the browser", serverLog.includes("effect: copy (16 characters)"), serverLog);
}

console.log("two-factor");
{
	const { session, page } = await openPopup("/totp.html");
	await evaluate(session, "document.querySelector('.row.fills').click()");
	await sleep(1200);
	const values = await fields(page);
	check("the code field gets a six-digit code", /^\d{6}$/.test(values.code ?? ""), JSON.stringify(values));
}

console.log("Vardo not running");
{
	server.kill("SIGTERM");
	await sleep(500);
	const { session } = await openPopup("/login.html");
	const text = await evaluate(session, "document.body.innerText");
	check("the popup says Vardo is not running", text.includes("Vardo isn't running"), text);
	check("and offers to start it", text.includes("Start Vardo"), text);
}

console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
site.close();
await finish(failures ? 1 : 0);
