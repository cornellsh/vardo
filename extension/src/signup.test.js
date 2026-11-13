// @vitest-environment jsdom
// @ts-nocheck

/** Putting a generated password into sign-up and password-change forms. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { signUpPage } from "./signup.js";

beforeEach(() => {
	vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
		const size = this.dataset.size === "0" ? 0 : 1;
		return { x: 10, y: 10, left: 10, top: 10, width: 200 * size, height: 30 * size, right: 210, bottom: 40 };
	});
});

afterEach(() => {
	document.body.innerHTML = "";
	vi.restoreAllMocks();
});

const value = (selector) => document.querySelector(selector).value;

describe("a sign-up form", () => {
	it("reports the address and the username typed so far, and types nothing", () => {
		document.body.innerHTML = `<form><input id="u" type="email"><input id="p" type="password"><input id="c" type="password"></form>`;
		document.querySelector("#u").value = "alex@example.com";
		expect(signUpPage(null, null)).toEqual({ url: location.href, username: "alex@example.com", fields: 2 });
		expect(value("#p")).toBe("");
	});

	it("fills the password and its confirmation alike", () => {
		document.body.innerHTML = `<form><input id="p" type="password"><input id="c" type="password"></form>`;
		expect(signUpPage(location.origin, "s3cret-Generated!")).toMatchObject({ fields: 2 });
		expect(value("#p")).toBe("s3cret-Generated!");
		expect(value("#c")).toBe("s3cret-Generated!");
	});

	it("uses the fields marked new-password when there are any", () => {
		document.body.innerHTML = `<form><input id="n" type="password" autocomplete="new-password"><input id="x" type="password"></form>`;
		signUpPage(location.origin, "fresh");
		expect(value("#n")).toBe("fresh");
		expect(value("#x")).toBe("");
	});
});

describe("a password-change form", () => {
	it("leaves the current password alone", () => {
		document.body.innerHTML = `<form><input id="old" type="password"><input id="new" type="password"><input id="again" type="password"></form>`;
		document.querySelector("#old").value = "current";
		signUpPage(location.origin, "fresh");
		expect([value("#old"), value("#new"), value("#again")]).toEqual(["current", "fresh", "fresh"]);
	});
});

describe("refusals", () => {
	it("finds nothing on a sign-in form", () => {
		document.body.innerHTML = `<form><input type="email"><input type="password"></form>`;
		expect(signUpPage(null, null)).toEqual({ reason: "no-sign-up" });
	});

	it("does not type for another origin", () => {
		document.body.innerHTML = `<form><input id="p" type="password"><input id="c" type="password"></form>`;
		expect(signUpPage("https://elsewhere.example", "x")).toEqual({ reason: "wrong-origin" });
		expect(value("#p")).toBe("");
	});

	it("ignores fields a person cannot see", () => {
		document.body.innerHTML = `<form><input type="password" data-size="0"><input type="password" data-size="0"></form>`;
		expect(signUpPage(null, null)).toEqual({ reason: "no-sign-up" });
	});
});
