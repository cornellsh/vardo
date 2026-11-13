import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { GroupNode } from "../ipc";
import { GroupRail } from "./GroupRail";

afterEach(cleanup);

const node = (uuid: string, name: string, children: GroupNode[] = []): GroupNode => ({
	uuid,
	name,
	children,
	entryCount: 0,
});
const root = node("root", "Database", [node("work", "Work"), node("home", "Home")]);
const TYPE = "application/x-vardo-group";

/** jsdom has no DataTransfer, so a drag carries this stand-in. */
function transfer(types: string[], payload = "") {
	const data: Record<string, string> = {};
	return {
		types,
		setData: (t: string, v: string) => (data[t] = v),
		getData: (t: string) => data[t] ?? payload,
		effectAllowed: "",
		dropEffect: "",
	};
}

function rail(onMoveGroup?: (uuid: string, parent: string) => void) {
	render(
		<GroupRail
			root={root}
			selected={undefined}
			onSelect={() => {}}
			totalEntries={0}
			vaultName="Vault"
			location="here"
			remote={false}
			onLock={() => {}}
			onSettings={() => {}}
			onHealth={() => {}}
			onMoveGroup={onMoveGroup}
		/>,
	);
	return (name: string) => screen.getByRole("button", { name: new RegExp(`^${name}`) });
}

describe("GroupRail dragging", () => {
	it("drops a group into another, or onto All entries for the top level", () => {
		const onMove = vi.fn();
		const row = rail(onMove);
		const drag = transfer([TYPE]);
		fireEvent.dragStart(row("Home"), { dataTransfer: drag });
		fireEvent.drop(row("Work"), { dataTransfer: drag });
		expect(onMove).toHaveBeenLastCalledWith("home", "work");

		fireEvent.drop(row("All entries"), { dataTransfer: transfer([TYPE], "work") });
		expect(onMove).toHaveBeenLastCalledWith("work", "root");
	});

	it("ignores a group dropped on itself, and anything that is not a group", () => {
		const onMove = vi.fn();
		const row = rail(onMove);
		fireEvent.drop(row("Work"), { dataTransfer: transfer([TYPE], "work") });
		fireEvent.drop(row("Work"), { dataTransfer: transfer(["text/uri-list"]) });
		expect(onMove).not.toHaveBeenCalled();
	});

	it("offers no drag on a read-only vault", () => {
		const row = rail();
		expect(row("Work").getAttribute("draggable")).toBeNull();
	});
});
