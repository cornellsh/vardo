import { describe, expect, it } from "vitest";

import conf from "../src-tauri/tauri.conf.json";

// `inv:no-phone-home`: the WebView may load nothing from outside the app.
describe("content security policy", () => {
	it("allows only the app itself, data images and IPC", () => {
		expect(conf.app.security.csp).toBe(
			"default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self'; script-src 'self'; connect-src 'self' ipc: http://ipc.localhost",
		);
	});
});
