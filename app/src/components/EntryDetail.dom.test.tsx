import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { EntryDetailPane } from "./EntryDetail";
import { ipc } from "../ipc";
import type { Ipc } from "../ipc";

/**
 * Rendering tests, as opposed to the pure-logic ones next door.
 *
 * The suite covered functions and nothing that draws, so a panel could throw on
 * mount and every check would still pass. These render real components against
 * the same mock the browser build uses.
 *
 * The first one is the only test here that would be worth writing on its own:
 * it asserts the property the whole IPC contract exists to protect.
 */

afterEach(cleanup);

/**
 * Unlocks the instance the components will actually use.
 *
 * `ipc()` is a singleton, and the first version of this constructed its own
 * `MockIpc`, unlocked that, and rendered a component which then reached for the
 * singleton and found it locked. The test was checking a different vault from
 * the one on screen.
 */
async function unlocked(): Promise<Ipc> {
	const api = await ipc();
	await api.openVault({ kind: "file", path: "/vaults/test.kdbx" }, "demopass");
	return api;
}

async function firstEntryUuid(api: Ipc): Promise<string> {
	const rows = await api.listEntries({});
	const row = rows[0];
	if (!row) throw new Error("the mock vault has no entries");
	return row.uuid;
}

describe("EntryDetailPane", () => {
	it("never puts a protected value in the document", async () => {
		// The contract in one assertion. A password reaches the clipboard through
		// Rust and reaches the screen only when someone presses Reveal; if one
		// ever arrives with the entry, it is in the DOM, in the accessibility
		// tree, and in any screenshot taken of the window.
		const api = await unlocked();
		const uuid = await firstEntryUuid(api);
		const detail = await api.getEntry(uuid);

		const password = detail.fields.find((f) => f.key === "Password");
		expect(password?.protected, "precondition: the field is protected").toBe(
			true,
		);
		expect(
			password?.value,
			"a protected field must arrive with no value at all",
		).toBeUndefined();

		render(
			<EntryDetailPane
				uuid={uuid}
				writable
				onChanged={() => {}}
				onSaveVault={async () => {}}
				editing={false}
				onEditing={() => {}}
			/>,
		);

		await waitFor(() => expect(screen.getByText(detail.title)).toBeTruthy());

		// The real secret, fetched separately, must not appear anywhere.
		const secret = await api.revealField(uuid, "Password");
		expect(secret.length).toBeGreaterThan(0);
		expect(document.body.textContent ?? "").not.toContain(secret);
	});

	it("shows an empty state rather than nothing when no entry is selected", async () => {
		await unlocked();
		render(
			<EntryDetailPane
				uuid={null}
				writable
				onChanged={() => {}}
				onSaveVault={async () => {}}
				editing={false}
				onEditing={() => {}}
			/>,
		);
		await waitFor(() =>
			expect(screen.getByText(/select an entry/i)).toBeTruthy(),
		);
	});

	it("renders an entry without throwing", async () => {
		// The blunt one. A panel that crashes on mount used to pass every check
		// in this repository, because nothing rendered it.
		const api = await unlocked();
		const uuid = await firstEntryUuid(api);
		const detail = await api.getEntry(uuid);

		render(
			<EntryDetailPane
				uuid={uuid}
				writable
				onChanged={() => {}}
				onSaveVault={async () => {}}
				editing={false}
				onEditing={() => {}}
			/>,
		);

		await waitFor(() => expect(screen.getByText(detail.title)).toBeTruthy());
		// The dots stand in for the password, so their presence is the evidence
		// the field rendered at all rather than being quietly dropped.
		expect(document.body.textContent).toContain("\u2022");
	});

	it("labels fields in words people use, not KeePass's key names", async () => {
		// Shipped as "UserName", directly beneath an editor that said
		// "Username". Found in a screenshot taken for the README.
		const api = await unlocked();
		// The mock is deliberately messy and plenty of its entries have no
		// username, which would hide the label entirely.
		const rows = await api.listEntries({});
		const withUsername = rows.find((r) => r.username);
		if (!withUsername) throw new Error("no mock entry has a username");
		const uuid = withUsername.uuid;

		render(
			<EntryDetailPane
				uuid={uuid}
				writable
				onChanged={() => {}}
				onSaveVault={async () => {}}
				editing={false}
				onEditing={() => {}}
			/>,
		);

		await waitFor(() => expect(screen.getByText("Username")).toBeTruthy());
		expect(document.body.textContent).not.toContain("UserName");
		// The screen-reader name too, which is where nobody looks.
		expect(screen.getByLabelText("Copy Username")).toBeTruthy();
	});
});
