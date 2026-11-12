import { describe, expect, it, vi } from "vitest";

import { saveThenLock } from "./saveThenLock";

describe("saveThenLock", () => {
	it("locks a clean vault without saving", async () => {
		const save = vi.fn(async () => {});
		const lock = vi.fn();
		expect(await saveThenLock(false, save, lock)).toBe(true);
		expect(save).not.toHaveBeenCalled();
		expect(lock).toHaveBeenCalledOnce();
	});

	it("saves unsaved edits before locking", async () => {
		const order: string[] = [];
		await saveThenLock(
			true,
			async () => {
				order.push("save");
			},
			() => order.push("lock"),
		);
		expect(order).toEqual(["save", "lock"]);
	});

	it("stays unlocked when the save fails", async () => {
		const lock = vi.fn();
		const failed = await saveThenLock(
			true,
			async () => {
				throw { kind: "io", detail: "disk full" };
			},
			lock,
		);
		expect(failed).toBe(false);
		expect(lock).not.toHaveBeenCalled();
	});
});
