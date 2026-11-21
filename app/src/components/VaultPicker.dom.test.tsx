import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { VaultPicker } from "./VaultPicker";
import type { VaultSource } from "../ipc";

afterEach(cleanup);

describe("VaultPicker", () => {
	it("goes from the Drive button to a chosen vault in one browser trip", async () => {
		const onSelect = vi.fn<(s: VaultSource) => void>();
		render(<VaultPicker selected={null} onSelect={onSelect} disabled={false} />);
		fireEvent.click(await screen.findByRole("button", { name: /Google Drive/ }));
		// No separate "Sign in" step: the browser is already on its way.
		await screen.findByText(/Choose your vault in your browser/);
		expect(screen.queryByRole("button", { name: "Sign in" })).toBeNull();
		await waitFor(
			() =>
				expect(onSelect).toHaveBeenCalledWith({
					kind: "googleDrive",
					file_id: "picked-1",
					name: "passwords.kdbx",
				}),
			{ timeout: 4000 },
		);
	});
});
