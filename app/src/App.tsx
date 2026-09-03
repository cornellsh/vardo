import { useCallback, useEffect, useState } from "react";

import { Unlock } from "./screens/Unlock";
import { Vault } from "./screens/Vault";
import type { VaultInfo } from "./ipc";
import { ipc } from "./ipc";

type State =
	| { kind: "locked"; reason?: "idle" | "manual" }
	| { kind: "unlocked"; info: VaultInfo };

export function App() {
	const [state, setState] = useState<State>({ kind: "locked" });

	const onUnlocked = useCallback((info: VaultInfo) => {
		setState({ kind: "unlocked", info });
	}, []);

	const onLock = useCallback(async (reason: "idle" | "manual") => {
		// Rust drops the plaintext DOM; the WebView only ever held metadata, so
		// there is nothing here to scrub beyond React state.
		(await ipc()).lock().catch(() => {});
		setState({ kind: "locked", reason });
	}, []);

	/**
	 * Keep the tray icon honest.
	 *
	 * Closing the window hides it rather than quitting, so an unlocked vault can
	 * sit behind no window at all, with nothing on screen to remind whoever
	 * walked away. The icon is the only thing left telling them, and an icon
	 * that lags the real state is worse than none: it would say locked while
	 * every password was one click away.
	 *
	 * Driven from here rather than read from the Rust session, because this is
	 * what knows about the moments in between - a merge rewriting the vault, an
	 * auto-lock that just fired, an unlock still deriving its key.
	 */
	useEffect(() => {
		void (async () => {
			const api = await ipc();
			await api.setTrayUnlocked?.(state.kind === "unlocked");
		})();
	}, [state.kind]);

	/** The tray's "Lock now", which is the whole point of it being there. */
	useEffect(() => {
		let dispose: (() => void) | undefined;
		void (async () => {
			const api = await ipc();
			dispose = await api.onLockRequested?.(() => {
				void onLock("manual");
			});
		})();
		return () => dispose?.();
	}, [onLock]);

	if (state.kind === "locked") {
		return <Unlock onUnlocked={onUnlocked} lockedReason={state.reason} />;
	}
	return <Vault info={state.info} onLock={onLock} />;
}
