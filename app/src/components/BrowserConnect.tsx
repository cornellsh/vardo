import { useCallback, useEffect, useState } from "react";

import { Notice, splitTechnical } from "./Notice";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { BrowserHosts } from "../ipc";

/**
 * Connecting the browser extension, without a terminal.
 *
 * Connecting writes one small file per browser that tells it where Vardo's
 * helper is. It gives the extension nothing: it still asks Vardo for every
 * login, and Vardo still decides.
 */
export function BrowserConnect() {
	const [hosts, setHosts] = useState<BrowserHosts | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		try {
			setHosts(await (await ipc()).browserHosts());
		} catch (err) {
			setError(errorText(err));
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	async function change(connect: boolean) {
		setBusy(true);
		setError(null);
		try {
			const api = await ipc();
			setHosts(await (connect ? api.browserConnect() : api.browserDisconnect()));
		} catch (err) {
			setError(errorText(err));
		} finally {
			setBusy(false);
		}
	}

	if (!hosts && !error) return null;
	const installed = hosts?.browsers.filter((b) => b.installed) ?? [];
	const connected = installed.filter((b) => b.connected);
	const anyConnected = connected.length > 0;

	return (
		<section
			className="rounded-[6px] p-4"
			style={{
				background: "var(--window-bg-over)",
				border: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="flex items-start gap-3">
				<div className="min-w-0 flex-1">
					<h3 className="font-semibold text-[var(--window-bold-fg)] text-[15px]">
						Browser extension
					</h3>
					<p className="text-[var(--window-sub-text-fg)]">
						{anyConnected
							? `Connected to ${connected.map((b) => b.name).join(", ")}`
							: "Fill passwords in Chrome, Brave, Edge and others"}
					</p>
				</div>

				{hosts?.helperFound && installed.length > 0 && (
					<button
						type="button"
						disabled={busy}
						onClick={() => void change(!anyConnected)}
						className="btn btn-quiet shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold transition-colors disabled:opacity-45"
						style={{
							background: anyConnected ? "transparent" : "var(--active-button-bg)",
							color: anyConnected ? "var(--attention-button-fg)" : "var(--window-fg-active)",
							transitionDuration: "var(--duration-universal)",
						}}
					>
						{anyConnected ? "Disconnect" : "Connect"}
					</button>
				)}
			</div>

			{hosts && !hosts.helperFound && (
				<p className="mt-3 text-[var(--window-sub-text-fg)]">
					Unavailable: this build has no browser helper beside it.
				</p>
			)}
			{hosts?.helperFound && installed.length === 0 && (
				<p className="mt-3 text-[var(--window-sub-text-fg)]">
					Start Chrome, Chromium, Brave, Edge or Vivaldi once, then come back.
				</p>
			)}
			{anyConnected && (
				<p className="mt-3 text-[var(--window-sub-text-fg)]">
					Install the Vardo extension in the browser if you have not yet.
				</p>
			)}

			{error &&
				(() => {
					const { message, technical } = splitTechnical(error);
					return (
						<div className="mt-3">
							<Notice title={message} {...(technical ? { technical } : {})} />
						</div>
					);
				})()}
		</section>
	);
}
