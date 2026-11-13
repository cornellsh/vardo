// @ts-nocheck
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { GECKO_ID, firefoxManifest } from "../scripts/firefox.mjs";

// `inv:extension-sees-nothing-until-asked`: any widening of what the extension
// can reach must fail here first.
const manifest = JSON.parse(readFileSync(join(import.meta.dirname, "..", "manifest.json"), "utf8"));

describe("manifest", () => {
	it("asks for exactly activeTab, scripting and nativeMessaging", () => {
		expect([...manifest.permissions].sort()).toEqual(["activeTab", "nativeMessaging", "scripting"]);
	});

	it("reaches no page on its own", () => {
		for (const key of [
			"host_permissions",
			"optional_permissions",
			"optional_host_permissions",
			"content_scripts",
			"web_accessible_resources",
			"externally_connectable",
		]) {
			expect(manifest, key).not.toHaveProperty(key);
		}
	});
});

describe("the Firefox manifest", () => {
	const firefox = firefoxManifest(manifest);

	it("asks for exactly what the Chrome one does, and reaches no page either", () => {
		expect(firefox.permissions).toEqual(manifest.permissions);
		expect(firefox.commands).toEqual(manifest.commands);
		for (const key of ["host_permissions", "optional_permissions", "content_scripts", "web_accessible_resources"]) {
			expect(firefox, key).not.toHaveProperty(key);
		}
	});

	it("runs the same background code as an event page, without Chrome-only keys", () => {
		expect(firefox.background).toEqual({ scripts: ["src/background.js"], type: "module" });
		expect(firefox).not.toHaveProperty("key");
		expect(firefox).not.toHaveProperty("minimum_chrome_version");
	});

	it("carries the add-on ID the native host is registered for", () => {
		const rust = readFileSync(join(import.meta.dirname, "..", "..", "crates", "vardo-browser", "src", "registration.rs"), "utf8");
		expect(rust).toContain(`pub const GECKO_ID: &str = "${GECKO_ID}";`);
		expect(firefox.browser_specific_settings.gecko.id).toBe(GECKO_ID);
	});
});
