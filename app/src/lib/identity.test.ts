import { describe, expect, it } from "vitest";

import { compactAge, hostOf, identityOf, initialOf, tintFor } from "./identity";

/**
 * These are not speculative. Every case here is a bug that shipped and was
 * caught by someone looking at the screen, which is a slow and unreliable way
 * to find out that eight days ago renders as "0mo".
 */

const DAY = 86_400_000;
const NOW = Date.UTC(2026, 8, 1);

describe("compactAge", () => {
	it("never says zero of anything", () => {
		// The bug as shipped: `Math.floor(days / 30)` is 0 for everything under
		// a month, so eight days old rendered as "0mo". Checked across the whole
		// range rather than at the two boundaries I happened to think of.
		for (let days = 0; days <= 800; days++) {
			const text = compactAge(NOW - days * DAY, NOW);
			expect(text, `${days} days`).not.toMatch(/^0/);
		}
	});

	it("moves through days, weeks, months, years", () => {
		expect(compactAge(NOW, NOW)).toBe("today");
		expect(compactAge(NOW - 1 * DAY, NOW)).toBe("1d");
		expect(compactAge(NOW - 6 * DAY, NOW)).toBe("6d");
		expect(compactAge(NOW - 7 * DAY, NOW)).toBe("1w");
		expect(compactAge(NOW - 29 * DAY, NOW)).toBe("4w");
		expect(compactAge(NOW - 30 * DAY, NOW)).toBe("1mo");
		expect(compactAge(NOW - 364 * DAY, NOW)).toBe("12mo");
		expect(compactAge(NOW - 365 * DAY, NOW)).toBe("1y");
	});

	it("treats a future timestamp as today rather than negative", () => {
		// Clock skew between two devices is ordinary, and "-3d" would be a
		// meaningless thing to show for it.
		expect(compactAge(NOW + 5 * DAY, NOW)).toBe("today");
	});
});

describe("identityOf", () => {
	it("gives one site one colour whether or not the URL is filled in", () => {
		// The bug as shipped: keying on the host first looked more precise and
		// meant that in a real vault, where some entries have a URL and some do
		// not, the same site came out in two colours.
		const withUrl = identityOf({ title: "Airbnb", url: "https://airbnb.com" });
		const without = identityOf({ title: "Airbnb", url: null });
		expect(withUrl).toBe(without);
		expect(tintFor(withUrl)).toBe(tintFor(without));
	});

	it("is case and whitespace insensitive", () => {
		expect(identityOf({ title: "  GitHub " })).toBe(
			identityOf({ title: "github" }),
		);
	});

	it("falls back to the host when there is no title", () => {
		expect(identityOf({ title: "", url: "https://www.example.com/x" })).toBe(
			"example.com",
		);
	});
});

describe("tintFor", () => {
	it("is stable, so a vault looks the same on every machine", () => {
		// A hash that varied per process would reshuffle every avatar on every
		// launch, which reads as the list being randomised.
		expect(tintFor("airbnb")).toBe(tintFor("airbnb"));
		expect(tintFor("airbnb")).toMatch(/^#[0-9a-f]{6}$/i);
	});

	it("spreads across the palette rather than collapsing onto one colour", () => {
		const names = ["airbnb", "github", "stripe", "monzo", "proton", "steam"];
		const distinct = new Set(names.map(tintFor));
		expect(distinct.size).toBeGreaterThan(1);
	});
});

describe("initialOf", () => {
	it("does not split a multi-byte character in half", () => {
		// `title[0]` cuts a surrogate pair and renders a replacement glyph. Real
		// vaults have emoji and CJK in titles.
		expect(initialOf("国民健康保険")).toBe("国");
		expect(initialOf("🎁 shared Netflix")).toBe("🎁");
	});

	it("uses two letters for a two-word name", () => {
		expect(initialOf("Proton Mail")).toBe("PM");
		expect(initialOf("Steam")).toBe("S");
	});

	it("has something to show for an untitled entry", () => {
		expect(initialOf("")).toBe("?");
		expect(initialOf("   ")).toBe("?");
	});
});

describe("hostOf", () => {
	it("accepts an address without a scheme", () => {
		// Vaults are full of bare hostnames typed by hand.
		expect(hostOf("example.com")).toBe("example.com");
		expect(hostOf("https://www.example.com/path?x=1")).toBe("example.com");
	});

	it("returns nothing for something that is not a web address", () => {
		expect(hostOf("")).toBe("");
		expect(hostOf("not a url")).toBe("");
	});
});
