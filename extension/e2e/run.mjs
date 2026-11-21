// The extension, end to end, in a real Chromium or Firefox.
//
//   npm run e2e            (from extension/)
//   npm run e2e:firefox
//
// Starts, in a private temporary directory:
//   - a small web server with a sign-in page and a two-factor page,
//   - Vardo's browser socket for a vault built by Vardo's own code
//     (`browser_e2e_server`, the exact server the app runs, without a window),
//   - the headless browser with this extension loaded and the native host
//     registered with it by `vardo-browser-host --install`.
//     chromium.mjs and firefox.mjs each drive one.
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
// Needs: cargo, and Chromium (set CHROMIUM if it is not `chromium`) or
// Firefox (set FIREFOX if it is not `firefox`).

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const extension = resolve(here, "..");
const repo = resolve(extension, "..");
const which = process.argv[2] ?? "chromium";
if (!["chromium", "firefox"].includes(which)) throw new Error(`unknown browser: ${which}`);
const driver = await import(`./${which}.mjs`);
const SITE_PORT = 18765;

const work = mkdtempSync(join(tmpdir(), "vardo-e2e-"));
const env = {
	...process.env,
	XDG_RUNTIME_DIR: join(work, "run"),
	XDG_CONFIG_HOME: join(work, "config"),
};
mkdirSync(env.XDG_RUNTIME_DIR, { mode: 0o700 });
// `--install` registers with each browser whose folder exists. On macOS those
// are under HOME, which XDG_CONFIG_HOME does not move, so the install gets its
// own HOME instead of writing into the real one.
const mac = process.platform === "darwin";
const home = join(work, "home");
mkdirSync(driver.profileDir({ home, env, mac }), { recursive: true });

const children = [];
const groups = new Set();
let failures = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function check(name, ok, detail = "") {
	console.log(`${ok ? "  ok  " : "  FAIL"} ${name}${ok ? "" : `\n         ${detail}`}`);
	if (!ok) failures++;
}

/**
 * Stops everything, waits for it to stop, then deletes the directory.
 *
 * A browser is not one process. Signalling only the one it started with left
 * Chromium's renderer and GPU helpers writing the profile while it was
 * deleted, so the delete failed after every check had passed. It runs in its
 * own process group, the whole group is signalled, and the delete retries. A
 * cleanup problem is reported but never turns a passing run into a failure.
 */
async function finish(code) {
	const exited = children
		.filter((child) => child.exitCode === null && child.signalCode === null)
		.map((child) => new Promise((r) => child.once("exit", r)));
	for (const child of children) {
		try {
			// A negative pid signals the group, for children started detached.
			process.kill(groups.has(child) ? -child.pid : child.pid, "SIGTERM");
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

const registered = spawnSync(host, ["--install"], { env: { ...env, HOME: home }, encoding: "utf8" });
const browserName = which === "firefox" ? "Firefox" : "Chromium";
// It reports on stderr: stdout is the channel to the browser.
check("the host registers itself with the browser", registered.status === 0 && registered.stderr.includes(`Registered with ${browserName}`), registered.stderr);

// --- the browser --------------------------------------------------------------------

let browser;
try {
	browser = await driver.start({
		extension,
		work,
		home,
		env,
		mac,
		track: (child) => {
			children.push(child);
			groups.add(child);
		},
	});
} catch (error) {
	console.error(error.message);
	await finish(1);
}

let opened = 0;

/**
 * Opens `path` in a new tab, clicks the toolbar button, and waits for the
 * popup to hear back from Vardo. Returns the popup and an evaluator for the page.
 */
async function openPopup(path) {
	// A unique address each time, so the tab and the page found are certainly
	// the ones just opened. The first version matched on the path, found an
	// older tab, and read back a page that was never filled.
	try {
		const opening = await browser.openPopup(`http://127.0.0.1:${SITE_PORT}${path}?run=${++opened}`);
		for (let i = 0; i < 40; i++) {
			await sleep(250);
			const text = await opening.popup.text();
			if (text && !/^\s*$/.test(text)) break;
		}
		await sleep(300);
		return opening;
	} catch (error) {
		// Through finish, or the browser outlives the run.
		console.error(error);
		return finish(1);
	}
}

const fields = (page) => page(`Object.fromEntries([...document.querySelectorAll('input')].map(i => [i.id, i.value]))`);

// --- the checks -----------------------------------------------------------------------

console.log("signing in");
{
	const { popup, page } = await openPopup("/login.html");
	const text = await popup.text();
	check("the popup lists the login saved for this site", text.includes("Test site"), text);
	check("a login saved for another port is not offered", !text.includes("Other service"), text);
	// Chrome writes it in each platform's own notation.
	check("the keyboard shortcut was assigned", /Alt\+Shift\+L|⌥⇧L/.test(text), text);

	await popup.choose();
	await sleep(1200);
	const values = await fields(page);
	check("the username is filled", values.email === "e2e-user@example.com", JSON.stringify(values));
	check("the password is filled", values.password === "e2e-password-123", JSON.stringify(values));
	check("the invisible trap form is left empty", values["trap-user"] === "" && values["trap-pass"] === "", JSON.stringify(values));
	check("the site's search box is left alone", values.search === "", JSON.stringify(values));
}

console.log("copying");
{
	const { popup } = await openPopup("/login.html");
	await popup.press("Copy password");
	await sleep(800);
	const text = await popup.text();
	check("the popup says the password was copied", text.includes("Password copied"), text);
	check("Vardo did the copying, not the browser", serverLog.includes("effect: copy (16 characters)"), serverLog);
}

console.log("frames");
{
	const { popup, page } = await openPopup("/frame.html");
	await sleep(500);
	await popup.choose();
	await sleep(1200);
	const frames = await page(
		`Object.fromEntries([...document.querySelectorAll("iframe")].map((f) => [f.id, { email: f.contentDocument.querySelector("#email").value, password: f.contentDocument.querySelector("#password").value }]))`,
	);
	const shown = JSON.stringify(frames);
	check("a login form inside a frame is filled", frames?.real?.email === "e2e-user@example.com" && frames?.real?.password === "e2e-password-123", shown);
	check("a frame made invisible is left empty", frames?.trap?.email === "" && frames?.trap?.password === "", shown);
}

console.log("two-factor");
{
	const { popup, page } = await openPopup("/totp.html");
	await popup.choose();
	await sleep(1200);
	const values = await fields(page);
	check("the code field gets a six-digit code", /^\d{6}$/.test(values.code ?? ""), JSON.stringify(values));
}

console.log("saving");
{
	const type = (page, email, password) =>
		page(`document.querySelector("#email").value = ${JSON.stringify(email)}; document.querySelector("#password").value = ${JSON.stringify(password)}`);
	let { popup, page } = await openPopup("/login.html");
	await type(page, "new-user@example.com", "brand-new-pw");
	await popup.press("Save the login");
	await sleep(800);
	let text = await popup.text();
	check("a typed login is saved to Vardo", text.includes("Saved to Vardo"), text);
	check("Vardo tells the app the vault changed", serverLog.includes("effect: changed"), serverLog);

	({ popup, page } = await openPopup("/login.html"));
	await type(page, "e2e-user@example.com", "rotated-pw");
	await popup.press("Save the login");
	await sleep(800);
	text = await popup.text();
	check("a different saved password is not replaced without asking", text.includes("Replace the saved password?"), text);
	await popup.press("Replace");
	await sleep(800);
	text = await popup.text();
	check("it is replaced once the person confirms", text.includes("Password updated in Vardo"), text);
}

console.log("making a password");
{
	const { popup, page } = await openPopup("/signup.html");
	await page(`document.querySelector("#email").value = "new-account@example.com"`);
	const before = (serverLog.match(/effect: changed/g) ?? []).length;
	await popup.press("Make a password");
	await sleep(1000);
	const text = await popup.text();
	const values = await fields(page);
	check("the popup says the password was made and kept", text.includes("New password typed in and kept in Vardo"), text);
	check("the password and its confirmation get the same new password", values.password.length === 20 && values.password === values.confirm, JSON.stringify({ length: values.password.length, same: values.password === values.confirm }));
	check("Vardo kept it before the form was sent", (serverLog.match(/effect: changed/g) ?? []).length === before + 1, serverLog);
}

console.log("Vardo not running");
{
	server.kill("SIGTERM");
	await sleep(500);
	const { popup } = await openPopup("/login.html");
	const text = await popup.text();
	check("the popup says Vardo is not running", text.includes("Vardo isn't running"), text);
	check("and offers to start it", text.includes("Start Vardo"), text);
}

console.log(failures ? `\n${failures} check(s) failed` : "\nall checks passed");
site.close();
await finish(failures ? 1 : 0);
