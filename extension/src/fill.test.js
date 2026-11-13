// @vitest-environment jsdom
// @ts-nocheck

/**
 * The code that types into pages, against the forms it will actually meet.
 *
 * jsdom has no layout engine, so element sizes are supplied here: every input
 * is 200 by 30 unless marked `data-size="0"`. Everything else visibility
 * depends on (display, visibility, opacity, inherited from ancestors) comes
 * from jsdom's real style computation.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { fillPage } from "./fill.js";

const LOGIN = { username: "alex@example.com", password: "correct-horse", totp: "492039" };

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

/** @param {string} html */
function page(html) {
	document.body.innerHTML = html;
}

/** @param {string} selector */
const value = (selector) => document.querySelector(selector).value;

const fill = (login = LOGIN) => fillPage(location.origin, login);

describe("an ordinary sign-in form", () => {
	it("fills the username and the password", () => {
		page(`<form><input id="u" type="email"><input id="p" type="password"><button>Sign in</button></form>`);
		expect(fill()).toEqual({ filled: ["username", "password"] });
		expect(value("#u")).toBe(LOGIN.username);
		expect(value("#p")).toBe(LOGIN.password);
	});

	it("tells the page's framework, the way typing would", () => {
		// React and friends listen for `input`. A value set silently is a value
		// they never learn about, and they submit the empty string instead.
		page(`<form><input id="u" type="text"><input id="p" type="password"></form>`);
		const seen = [];
		document.querySelector("#p").addEventListener("input", (e) => seen.push(e.target.value));
		fill();
		expect(seen).toEqual([LOGIN.password]);
	});

	it("never submits the form", () => {
		page(`<form><input type="text"><input type="password"></form>`);
		const submitted = vi.fn((e) => e.preventDefault());
		document.querySelector("form").addEventListener("submit", submitted);
		fill();
		expect(submitted).not.toHaveBeenCalled();
	});

	it("leaves the cursor in the password field, so Enter signs in", () => {
		page(`<form><input type="text"><input id="p" type="password"></form>`);
		fill();
		expect(document.activeElement).toBe(document.querySelector("#p"));
	});

	it("leaves a search box outside the form alone", () => {
		page(`
			<header><input id="search" type="text" placeholder="Search"></header>
			<form><input id="u" type="text"><input id="p" type="password"></form>`);
		fill();
		expect(value("#search")).toBe("");
		expect(value("#u")).toBe(LOGIN.username);
	});

	it("prefers the field marked as the username", () => {
		page(`<form>
			<input id="u" autocomplete="username" type="text">
			<input id="other" type="text" name="remember-code">
			<input id="p" type="password"></form>`);
		fill();
		expect(value("#u")).toBe(LOGIN.username);
		expect(value("#other")).toBe("");
	});
});

describe("forms that only look like sign-in forms", () => {
	it("does not put the current password into a sign-up form", () => {
		page(`<form><input type="email">
			<input id="a" type="password" autocomplete="new-password">
			<input id="b" type="password" autocomplete="new-password"></form>`);
		expect(fill()).toEqual({ filled: [], reason: "new-password" });
		expect(value("#a")).toBe("");
		expect(value("#b")).toBe("");
	});

	it("fills only the current password when changing it", () => {
		page(`<form>
			<input id="now" type="password" autocomplete="current-password">
			<input id="new" type="password" autocomplete="new-password">
			<input id="again" type="password" autocomplete="new-password"></form>`);
		fill();
		expect(value("#now")).toBe(LOGIN.password);
		expect(value("#new")).toBe("");
		expect(value("#again")).toBe("");
	});
});

describe("fields a person cannot see", () => {
	// The 2025 clickjacking attacks collected logins through forms made almost
	// transparent. Filling only what someone could see closes that.

	it("ignores a nearly transparent form and fills the visible one", () => {
		page(`
			<form id="trap" style="opacity: 0.01"><input id="tu" type="text"><input id="tp" type="password"></form>
			<form><input id="u" type="text"><input id="p" type="password"></form>`);
		fill();
		expect(value("#tp")).toBe("");
		expect(value("#tu")).toBe("");
		expect(value("#p")).toBe(LOGIN.password);
	});

	it("fills nothing when the only form is invisible", () => {
		for (const style of ["opacity: 0", "display: none", "visibility: hidden"]) {
			page(`<div style="${style}"><form><input id="u" type="text"><input id="p" type="password"></form></div>`);
			expect(fill(), style).toEqual({ filled: [], reason: "no-fields" });
			expect(value("#p"), style).toBe("");
		}
	});

	it("ignores a zero-size honeypot field", () => {
		page(`<form><input id="u" type="text"><input id="hp" type="password" data-size="0"><input id="p" type="password"></form>`);
		fill();
		expect(value("#hp")).toBe("");
		expect(value("#p")).toBe(LOGIN.password);
	});

	it("ignores fields that are disabled or read-only", () => {
		page(`<form><input id="u" type="text" readonly><input id="p" type="password" disabled></form>`);
		expect(fill().filled).toEqual([]);
	});
});

describe("sign-in in more than one step", () => {
	it("fills the username when that is all the page asks for", () => {
		page(`<form><input id="id" type="email" autocomplete="username"><button>Next</button></form>`);
		expect(fill()).toEqual({ filled: ["username"] });
		expect(value("#id")).toBe(LOGIN.username);
	});

	it("fills the two-factor code on the page that asks for it", () => {
		page(`<form><input id="code" type="text" inputmode="numeric" autocomplete="one-time-code"></form>`);
		expect(fill()).toEqual({ filled: ["totp"] });
		expect(value("#code")).toBe(LOGIN.totp);
	});

	it("recognises a code field by its name when it is not marked", () => {
		page(`<form><input id="code" type="text" name="otp_code"></form>`);
		fill();
		expect(value("#code")).toBe(LOGIN.totp);
	});

	it("fills one digit per box when the code is split up", () => {
		page(`<form>${Array.from({ length: 6 }, (_, i) => `<input id="d${i}" type="text" maxlength="1">`).join("")}</form>`);
		expect(fill()).toEqual({ filled: ["totp"] });
		expect([0, 1, 2, 3, 4, 5].map((i) => value(`#d${i}`)).join("")).toBe(LOGIN.totp);
	});

	it("does not mistake a code field for the username", () => {
		page(`<form><input id="code" type="text" autocomplete="one-time-code"></form>`);
		fill({ ...LOGIN, totp: null });
		expect(value("#code")).toBe("");
	});
});

describe("where it runs", () => {
	it("does nothing if the page is no longer the one Vardo approved", () => {
		page(`<form><input id="u" type="text"><input id="p" type="password"></form>`);
		expect(fillPage("https://github.com", LOGIN)).toEqual({ filled: [], reason: "wrong-origin" });
		expect(value("#p")).toBe("");
	});

	it("refers to nothing outside itself, because Chrome sends only its source", () => {
		// `executeScript({ func })` serialises the function's text. A reference
		// to a helper defined elsewhere in this module would be undefined in the
		// page and throw on first use.
		const isolated = new Function(`return (${fillPage.toString()})`)();
		page(`<form><input id="u" type="text"><input id="p" type="password"></form>`);
		expect(isolated(location.origin, LOGIN)).toEqual({ filled: ["username", "password"] });
	});
});

describe("a dry run", () => {
	it("reports what it would fill and types nothing", () => {
		page(`<form><input id="u" type="email"><input id="p" type="password"></form>`);
		expect(fillPage(null, null)).toEqual({ filled: ["username", "password"] });
		expect(value("#u")).toBe("");
		expect(value("#p")).toBe("");
	});

	it("offers nothing on a page without login fields", () => {
		page(`<input id="q" type="search">`);
		expect(fillPage(null, null)).toEqual({ filled: [], reason: "no-fields" });
	});
});
