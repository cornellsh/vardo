import { describe, expect, it } from "vitest";

import { splitTechnical } from "../components/Notice";
import { modeFor } from "./useLayoutMode";

describe("modeFor", () => {
	it("drops a pane before the panes reach their minimum", () => {
		// The arithmetic minimum for three panes is 260 + 380 + 292 = 932. The
		// threshold sits above it because a pane at exactly its minimum is
		// technically present and practically unusable.
		expect(modeFor(1400)).toBe("three");
		expect(modeFor(1000)).toBe("three");
		expect(modeFor(999)).toBe("two");
		expect(modeFor(700)).toBe("two");
		expect(modeFor(699)).toBe("one");
		expect(modeFor(360)).toBe("one");
	});

	it("never leaves three panes in a window too narrow for them", () => {
		// The failure as shipped: below 932 the app did not adapt, it
		// overflowed, and at 600 the detail pane was 332 pixels past the edge
		// with no scrollbar to reach it.
		for (let w = 200; w < 932; w++) {
			expect(modeFor(w), `${w}px`).not.toBe("three");
		}
	});

	it("holds two panes only where two actually fit", () => {
		// list 380 + detail 292 = 672.
		for (let w = 200; w < 672; w++) {
			expect(modeFor(w), `${w}px`).toBe("one");
		}
	});

	it("survives a nonsense width rather than throwing", () => {
		// A window manager reporting zero mid-resize is not worth crashing over.
		expect(modeFor(0)).toBe("one");
		expect(modeFor(-1)).toBe("one");
	});
});

describe("splitTechnical", () => {
	it("moves a trailing cause behind the disclosure", () => {
		const { message, technical } = splitTechnical(
			"Could not reach Google Drive (dns failure)",
		);
		expect(message).toBe("Could not reach Google Drive");
		expect(technical).toBe("dns failure");
	});

	it("leaves a message with no parenthetical alone", () => {
		const { message, technical } = splitTechnical("Wrong password");
		expect(message).toBe("Wrong password");
		expect(technical).toBeUndefined();
	});

	it("does not eat parentheses from the middle of a sentence", () => {
		// "vault (1).kdbx" appears in our own copy about conflicted copies, and
		// splitting on it would leave the message ending mid-thought.
		const { message, technical } = splitTechnical(
			'A second file such as "vault (1).kdbx" may appear',
		);
		expect(message).toBe('A second file such as "vault (1).kdbx" may appear');
		expect(technical).toBeUndefined();
	});
});
