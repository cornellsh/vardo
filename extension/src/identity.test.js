// @ts-nocheck

/**
 * The popup's copy of the avatar rules must agree with the app's, or a site is
 * green in Vardo and pink in the browser.
 */

import { describe, expect, it } from "vitest";

import * as app from "../../app/src/lib/identity.ts";
import * as extension from "./identity.js";

const TITLES = ["GitHub", "github", "  Airbnb ", "Deutsche Bank", "🎁 shared Netflix", "北京银行", "", "AWS root"];
const URLS = [null, "", "github.com", "https://www.amazon.com/ap/signin", "not a url", "http://[::1]:8080/"];

describe("the popup draws logins the way Vardo does", () => {
	it("gives every login the same colour", () => {
		for (const title of TITLES) {
			for (const url of URLS) {
				const id = app.identityOf({ title, url });
				expect(extension.identityOf({ title, url }), `${title} ${url}`).toBe(id);
				expect(extension.tintFor(id), id).toBe(app.tintFor(id));
			}
		}
	});

	it("gives every login the same initial", () => {
		for (const title of TITLES) {
			expect(extension.initialOf(title), title).toBe(app.initialOf(title));
		}
	});

	it("reads hosts the same way", () => {
		for (const url of URLS) {
			expect(extension.hostOf(url ?? ""), String(url)).toBe(app.hostOf(url ?? ""));
		}
	});
});
