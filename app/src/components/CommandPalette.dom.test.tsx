import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CommandPalette } from "./CommandPalette";
import type { Command } from "./CommandPalette";

afterEach(cleanup);

function setup() {
	const ran: string[] = [];
	const commands: Command[] = ["New entry", "Lock now", "Password health"].map((label) => ({
		id: label,
		label,
		run: () => ran.push(label),
	}));
	const onClose = vi.fn();
	render(<CommandPalette commands={commands} onClose={onClose} />);
	return { ran, onClose, box: screen.getByRole("textbox", { name: "Type a command" }) };
}

const labels = () => screen.getAllByRole("option").map((o) => o.textContent);

describe("CommandPalette", () => {
	it("lists every command and narrows as words are typed, in any order", () => {
		const { box } = setup();
		expect(labels()).toEqual(["New entry", "Lock now", "Password health"]);
		fireEvent.change(box, { target: { value: "health pass" } });
		expect(labels()).toEqual(["Password health"]);
	});

	it("runs the selected command on Enter and closes", () => {
		const { box, ran, onClose } = setup();
		fireEvent.keyDown(box, { key: "ArrowDown" });
		fireEvent.keyDown(box, { key: "Enter" });
		expect(ran).toEqual(["Lock now"]);
		expect(onClose).toHaveBeenCalled();
	});

	it("closes on Escape without running anything", () => {
		const { box, ran, onClose } = setup();
		fireEvent.keyDown(box, { key: "Escape" });
		expect(onClose).toHaveBeenCalled();
		expect(ran).toEqual([]);
	});

	it("says so when nothing matches, and Enter then does nothing", () => {
		const { box, ran } = setup();
		fireEvent.change(box, { target: { value: "launch rockets" } });
		expect(screen.getByText("No command matches.")).toBeTruthy();
		fireEvent.keyDown(box, { key: "Enter" });
		expect(ran).toEqual([]);
	});
});
