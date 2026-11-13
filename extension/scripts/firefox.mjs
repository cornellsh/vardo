// @ts-nocheck: a Node build script; the extension's type check has no Node types.
// Builds the Firefox copy of the extension in dist/firefox.
//
// The code is shared. Only the manifest differs: Firefox runs MV3 background
// code as an event page rather than a service worker, names the extension by
// add-on ID, and has no use for Chrome's public key. Run: npm run build:firefox
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** Must match GECKO_ID in crates/vardo-browser/src/registration.rs. */
export const GECKO_ID = "browser@vardo.app";

/** @param {Record<string, any>} chrome The Chrome manifest. */
export function firefoxManifest(chrome) {
	const { key: _key, minimum_chrome_version: _min, background, ...rest } = chrome;
	return {
		...rest,
		background: { scripts: [background.service_worker], type: background.type },
		browser_specific_settings: {
			// 128 is the first release with MV3 activeTab granted by a keyboard command.
			gecko: {
				id: GECKO_ID,
				strict_min_version: "128.0",
				// Nothing leaves the machine (dec:no-server-no-telemetry).
				data_collection_permissions: { required: ["none"] },
			},
		},
	};
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const out = join(root, "dist", "firefox");
	rmSync(out, { recursive: true, force: true });
	mkdirSync(out, { recursive: true });
	cpSync(join(root, "icons"), join(out, "icons"), { recursive: true });
	cpSync(join(root, "src"), join(out, "src"), {
		recursive: true,
		filter: (path) => !path.endsWith(".test.js"),
	});
	let chrome;
	try {
		chrome = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));
	} catch (error) {
		throw new Error(`manifest.json could not be read as JSON: ${error}`);
	}
	writeFileSync(join(out, "manifest.json"), `${JSON.stringify(firefoxManifest(chrome), null, 2)}\n`);
	console.log(`wrote ${out}`);
}
