/**
 * Picks the real Rust core when running inside Tauri, and the mock when running
 * in a plain browser via `npm run dev`.
 *
 * The mock exists so the UI can be built and reviewed before the Tauri shell is
 * available, but it earns its keep permanently: it is the only way to exercise
 * the interface against a 400-entry vault without unlocking a real one.
 */

import { MockIpc } from "./mock";
import type { Ipc } from "./types";

declare global {
	interface Window {
		__TAURI_INTERNALS__?: unknown;
	}
}

export const isTauri = (): boolean =>
	typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;

let instance: Ipc | null = null;

export async function ipc(): Promise<Ipc> {
	if (instance) return instance;
	if (isTauri()) {
		// Dynamic, so the browser build never pulls in the Tauri API bundle.
		const { TauriIpc } = await import("./tauri");
		instance = new TauriIpc();
	} else {
		instance = new MockIpc();
	}
	return instance;
}

export * from "./types";
