import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ipc } from "../ipc";
import type { Ipc } from "../ipc";
import { UpdatePrompt, UpdateSettings } from "./Updates";

let api: Ipc;

beforeEach(async () => {
	api = await ipc();
});

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("Updates", () => {
	it("asks before checking, and offers the restart once the update is verified", async () => {
		const check = vi.spyOn(api, "checkForUpdate");
		render(<UpdatePrompt />);
		await screen.findByText("Check for updates once a day?");
		expect(check).not.toHaveBeenCalled();

		fireEvent.click(screen.getByRole("button", { name: "Check daily" }));
		await screen.findByText("Vardo 0.0.2 is ready", {}, { timeout: 3000 });
		expect(screen.queryByText("Check for updates once a day?")).toBeNull();
		expect(screen.getByRole("button", { name: "Restart to update" })).toBeTruthy();
	});

	it("says why it will not restart over unsaved changes", async () => {
		vi.spyOn(api, "updateStatus").mockResolvedValue({
			current: "0.0.1",
			checks: "on",
			state: { kind: "ready", version: "0.0.2", notes: "" },
		});
		vi.spyOn(api, "installUpdate").mockRejectedValue({ kind: "unsaved-changes" });
		render(<UpdateSettings />);
		fireEvent.click(await screen.findByRole("button", { name: "Restart to update" }));
		await screen.findByText("Save your changes first. Restarting would lose them.");
	});
});
