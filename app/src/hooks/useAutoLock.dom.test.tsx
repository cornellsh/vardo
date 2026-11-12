import { cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useAutoLock } from "./useAutoLock";

const MINUTE = 60_000;

beforeEach(() => {
	vi.useFakeTimers();
	vi.setSystemTime(new Date("2026-01-01T12:00:00Z"));
});

afterEach(() => {
	cleanup();
	vi.useRealTimers();
});

describe("useAutoLock", () => {
	it("locks once after five idle minutes", () => {
		const onLock = vi.fn();
		renderHook(() => useAutoLock(5, onLock));
		vi.advanceTimersByTime(5 * MINUTE - 2000);
		expect(onLock).not.toHaveBeenCalled();
		vi.advanceTimersByTime(3000);
		expect(onLock).toHaveBeenCalledOnce();
		vi.advanceTimersByTime(10 * MINUTE);
		expect(onLock).toHaveBeenCalledOnce();
	});

	it("restarts the five minutes on input", () => {
		const onLock = vi.fn();
		renderHook(() => useAutoLock(5, onLock));
		vi.advanceTimersByTime(4 * MINUTE);
		window.dispatchEvent(new KeyboardEvent("keydown"));
		vi.advanceTimersByTime(4 * MINUTE);
		expect(onLock).not.toHaveBeenCalled();
		vi.advanceTimersByTime(MINUTE + 1000);
		expect(onLock).toHaveBeenCalledOnce();
	});

	it("locks on waking when the machine slept through the window", () => {
		const onLock = vi.fn();
		renderHook(() => useAutoLock(5, onLock));
		// A suspended machine fires no timers, so only the clock moves.
		vi.setSystemTime(Date.now() + 30 * MINUTE);
		document.dispatchEvent(new Event("visibilitychange"));
		expect(onLock).toHaveBeenCalledOnce();
	});
});
