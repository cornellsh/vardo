import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MockInstance } from "vitest";

import { ipc } from "../ipc";
import type { Ipc } from "../ipc";
import { Quick } from "./Quick";

let api: Ipc;
let done: ReturnType<typeof vi.fn<(revealMain: boolean) => Promise<void>>>;
let copy: MockInstance<Ipc["copyField"]>;

beforeEach(async () => {
	api = await ipc();
	await api.openVault({ kind: "file", path: "/vaults/test.kdbx" }, "demopass");
	done = vi.fn(async (_revealMain: boolean) => {});
	api.quickDone = done;
	copy = vi.spyOn(api, "copyField");
});

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

async function search(text: string) {
	render(<Quick />);
	const box = screen.getByRole("textbox", { name: "Search logins" });
	fireEvent.change(box, { target: { value: text } });
	await waitFor(() => expect(screen.getAllByRole("option").length).toBeGreaterThan(0));
	return box;
}

describe("Quick", () => {
	it("copies the first match's password on Enter, then closes", async () => {
		const box = await search("airbnb");
		const first = (await api.listEntries({ query: "airbnb" }))[0];
		fireEvent.keyDown(box, { key: "Enter" });
		await waitFor(() => expect(done).toHaveBeenCalledWith(false));
		expect(copy).toHaveBeenCalledWith(first?.uuid, "Password");
	});

	it("copies the username on Shift+Enter", async () => {
		const box = await search("airbnb");
		fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
		await waitFor(() => expect(copy).toHaveBeenCalledWith(expect.any(String), "UserName"));
	});

	it("moves the selection with the arrow keys", async () => {
		const box = await search("airbnb");
		const rows = await api.listEntries({ query: "airbnb" });
		fireEvent.keyDown(box, { key: "ArrowDown" });
		fireEvent.keyDown(box, { key: "Enter" });
		await waitFor(() => expect(copy).toHaveBeenCalledWith(rows[1]?.uuid, "Password"));
	});

	it("closes on Escape without copying", async () => {
		const box = await search("airbnb");
		fireEvent.keyDown(box, { key: "Escape" });
		await waitFor(() => expect(done).toHaveBeenCalledWith(false));
		expect(copy).not.toHaveBeenCalled();
	});

	it("sends a locked vault to the main window to unlock", async () => {
		await api.lock();
		render(<Quick />);
		await screen.findByText("Press Enter to unlock it in the main window.");
		fireEvent.keyDown(screen.getByRole("textbox", { name: "Search logins" }), { key: "Enter" });
		await waitFor(() => expect(done).toHaveBeenCalledWith(true));
		expect(copy).not.toHaveBeenCalled();
	});
});
