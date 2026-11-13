import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ipc } from "../ipc";
import type { Ipc } from "../ipc";
import { IconFetch } from "./IconFetch";

let api: Ipc;

beforeEach(async () => {
	api = await ipc();
	await api.openVault({ kind: "file", path: "/vaults/test.kdbx" }, "demopass");
});

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("IconFetch", () => {
	it("says how many sites it will ask before asking any, then saves once", async () => {
		vi.spyOn(api, "entriesMissingIcons")
			.mockResolvedValueOnce(["a", "b", "c"])
			.mockResolvedValue(["c"]);
		const fetchIcon = vi
			.spyOn(api, "fetchIcon")
			.mockImplementation(async (uuid) => (uuid === "c" ? null : "data:image/png;base64,AA"));
		const onFinished = vi.fn(async () => {});

		render(<IconFetch onProgress={() => {}} onFinished={onFinished} />);
		await screen.findByText(/3 entries have no icon yet/);
		expect(fetchIcon).not.toHaveBeenCalled();

		fireEvent.click(screen.getByRole("button", { name: "Get 3 icons" }));
		await screen.findByText("Found 2 of 3. The other sites don't offer one.");
		expect(fetchIcon).toHaveBeenCalledTimes(3);
		expect(onFinished).toHaveBeenCalledTimes(1);
	});

	it("offers nothing when every entry already has an icon", async () => {
		vi.spyOn(api, "entriesMissingIcons").mockResolvedValue([]);
		render(<IconFetch onProgress={() => {}} onFinished={async () => {}} />);
		await screen.findByText("Every entry with a web address has an icon.");
		expect(screen.queryByRole("button")).toBeNull();
	});
});
