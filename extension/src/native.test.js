// @ts-nocheck
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import {
	COPY_FIELDS,
	ERROR_CODES,
	REPLIES,
	REQUESTS,
	STATE_KINDS,
	understood,
} from "./native.js";

// Written by crates/vardo-browser/tests/protocol_fixtures.rs from the Rust
// types. When Rust renames or adds a message, these tests fail until
// native.js says the same.
const dir = join(import.meta.dirname, "..", "protocol");
/** @param {string} sub */
const read = (sub) =>
	readdirSync(join(dir, sub)).map((file) => ({
		name: file.replace(/\.json$/, ""),
		message: JSON.parse(readFileSync(join(dir, sub, file), "utf8")),
	}));

/** @param {Record<string, unknown>} message */
const fields = (message) =>
	Object.keys(message)
		.filter((k) => !["v", "id", "type"].includes(k))
		.sort();

describe("the protocol as Rust writes it", () => {
	const requests = read("requests");
	const replies = read("responses");
	const enums = JSON.parse(readFileSync(join(dir, "enums.json"), "utf8"));

	it("has a request for every one the extension sends, with the same fields", () => {
		expect(requests.map((r) => r.name).sort()).toEqual(Object.keys(REQUESTS).sort());
		for (const { name, message } of requests) {
			expect(message.type).toBe(name);
			expect(fields(message)).toEqual([...REQUESTS[name]].sort());
		}
	});

	it("has a reply for every one the extension understands, with the same fields", () => {
		expect(replies.map((r) => r.name).sort()).toEqual(Object.keys(REPLIES).sort());
		for (const { name, message } of replies) {
			expect(message.type).toBe(name);
			expect(fields(message)).toEqual([...REPLIES[name]].sort());
			expect(understood(message)).toBe(true);
		}
	});

	it("uses the error codes, copy fields and states the extension knows", () => {
		expect(enums["error-code"]).toEqual(ERROR_CODES);
		expect(enums["copy-field"]).toEqual(COPY_FIELDS);
		expect(enums.state.map((s) => s.kind)).toEqual(STATE_KINDS);
	});

	it("does not understand a reply type Rust never sends", () => {
		expect(understood({ type: "filled-in" })).toBe(false);
		expect(understood({ type: "fill", username: "a" })).toBe(false);
		expect(understood(null)).toBe(false);
	});
});
