import { useCallback, useState } from "react";

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

	if (state.kind === "locked") {
		return <Unlock onUnlocked={onUnlocked} lockedReason={state.reason} />;
	}
	return <Vault info={state.info} onLock={onLock} />;
}
