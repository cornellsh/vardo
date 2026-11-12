// @ts-nocheck
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

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
