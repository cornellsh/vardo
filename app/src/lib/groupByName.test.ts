import { describe, expect, it } from "vitest";

import type { GroupNode } from "../ipc";
import { findGroup, groupByName } from "./groupByName";

const node = (uuid: string, name: string, children: GroupNode[] = []): GroupNode =>
	({ uuid, name, children, entryCount: 0 }) as GroupNode;

const root = node("root", "Database", [
	node("work", "Work", [node("acme", "Acme")]),
	node("home", "Home", [node("acme2", "acme")]),
	node("bank", "Banks"),
]);

describe("findGroup", () => {
	it("finds a group at any depth, and nothing for an unknown id", () => {
		expect(findGroup(root, "acme2")?.name).toBe("acme");
		expect(findGroup(root, "gone")).toBeUndefined();
	});
});

describe("groupByName", () => {
	it("finds a nested group whatever the case", () => {
		expect(groupByName(root, "  banks ")).toMatchObject({ uuid: "bank" });
		expect(groupByName(root, "WORK")).toMatchObject({ uuid: "work" });
	});

	it("takes an empty name as the top level", () => {
		expect(groupByName(root, "")).toBe(root);
	});

	it("refuses a name it cannot place, rather than guess", () => {
		expect(groupByName(root, "Nope")).toBe("No group is called Nope.");
		expect(groupByName(root, "acme")).toMatch(/More than one group is called acme/);
		expect(groupByName(root, "Database")).toBe("No group is called Database.");
	});
});
