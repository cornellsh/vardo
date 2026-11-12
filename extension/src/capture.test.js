// @vitest-environment jsdom
// @ts-nocheck

/**
 * Reading a typed login back out of the page, for saving.
 *
 * jsdom has no layout, so every input is 200 by 30 unless `data-size="0"`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { capturePage } from "./capture.js";

beforeEach(() => {
	vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function () {
		const size = this.dataset.size === "0" ? 0 : 1;
		return { x: 10, y: 10, left: 10, top: 10, width: 200 * size, height: 30 * size, right: 10 + 200 * size, bottom: 10 + 30 * size };
	});
});

afterEach(() => {
	document.body.innerHTML = "";
	vi.restoreAllMocks();
});

/** Builds the page, then types each `[selector, value]` as a person would. */
function page(html, typed) {
	document.body.innerHTML = html;
	for (const [selector, value] of typed) document.querySelector(selector).value = value;
}

describe("capturing a typed login", () => {
	it("reads the username and password of a sign-in form", () => {
		page(`<form><input id="u" type="email"><input id="p" type="password"></form>`, [
			["#u", " alex@example.com "],
			["#p", "correct-horse"],
		]);
		expect(capturePage()).toEqual({ url: location.href, username: "alex@example.com", password: "correct-horse" });
	});

	it("says so when no password was typed", () => {
		page(`<form><input id="u" type="text"><input id="p" type="password"></form>`, [["#u", "alex"]]);
		expect(capturePage()).toEqual({ reason: "no-password" });
	});

	it("takes the new password on a change-password form", () => {
		page(
			`<form><input id="old" type="password"><input id="new" type="password"><input id="again" type="password"></form>`,
			[["#old", "old-secret"], ["#new", "new-secret"], ["#again", "new-secret"]],
		);
		expect(capturePage().password).toBe("new-secret");
	});

	it("prefers the field marked new-password", () => {
		page(
			`<form><input id="new" type="password" autocomplete="new-password"><input id="old" type="password"></form>`,
			[["#new", "new-secret"], ["#old", "old-secret"]],
		);
		expect(capturePage().password).toBe("new-secret");
	});

	it("prefers a field marked username over a nearer text field", () => {
		page(
			`<form><input id="u" autocomplete="username"><input id="code" type="text"><input id="p" type="password"></form>`,
			[["#u", "alex"], ["#code", "1234"], ["#p", "pw"]],
		);
		expect(capturePage().username).toBe("alex");
	});

	it("ignores a password hidden from the person", () => {
		page(`<form><input id="trap" type="password" data-size="0"></form>`, [["#trap", "planted"]]);
		expect(capturePage()).toEqual({ reason: "no-password" });
	});

	it("reads only the form the person is typing in", () => {
		page(
			`<form id="a"><input id="ua" type="text"><input id="pa" type="password"></form>
			 <form id="b"><input id="ub" type="text"><input id="pb" type="password"></form>`,
			[["#ua", "first"], ["#pa", "one"], ["#ub", "second"], ["#pb", "two"]],
		);
		document.querySelector("#pb").focus();
		expect(capturePage()).toMatchObject({ username: "second", password: "two" });
	});
});
