import { useCallback, useEffect, useState } from "react";

import { DriveLogo } from "./DriveLogo";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { DriveStatus } from "../ipc";

/**
 * Connecting a Google account.
 *
 * The copy does the work here. "Sign in with Google" tells a user nothing about
 * what the app will be able to see, and for a password manager that is exactly
 * the question they should be asking. So the scope is stated plainly, before
 * the button, in terms of what it means rather than what it is called.
 */
export function DriveConnect() {
	const [status, setStatus] = useState<DriveStatus | null>(null);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const refresh = useCallback(async () => {
		try {
			setStatus(await (await ipc()).driveStatus());
		} catch (err) {
			setError(errorText(err));
		}
	}, []);

	useEffect(() => {
		void refresh();
	}, [refresh]);

	async function connect() {
		setBusy(true);
		setError(null);
		try {
			await (await ipc()).driveConnect();
			await refresh();
		} catch (err) {
			setError(errorText(err));
		} finally {
			setBusy(false);
		}
	}

	async function disconnect() {
		setBusy(true);
		try {
			await (await ipc()).driveDisconnect();
			await refresh();
		} catch (err) {
			setError(errorText(err));
		} finally {
			setBusy(false);
		}
	}

	if (!status) return null;

	return (
		<section
			className="rounded-[6px] p-4"
			style={{
				background: "var(--window-bg-over)",
				border: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="flex items-start gap-3">
				<DriveLogo size={32} muted={!status.connected} />

				<div className="min-w-0 flex-1">
					<h3 className="font-semibold text-[var(--window-bold-fg)] text-[15px]">
						Google Drive
					</h3>

					{status.connected ? (
						<p className="text-[var(--window-sub-text-fg)]">
							Connected. Vardo can reach the vault files you pick, and nothing
							else in your Drive.
						</p>
					) : (
						<p className="text-[var(--window-sub-text-fg)]">
							Sync this vault through Drive&rsquo;s own API instead of a synced
							folder, so two devices editing at once are merged rather than
							overwriting each other.
						</p>
					)}
				</div>

				{status.configured && (
					<button
						type="button"
						disabled={busy}
						onClick={() => void (status.connected ? disconnect() : connect())}
						className="shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold transition-colors disabled:opacity-45"
						style={{
							background: status.connected
								? "transparent"
								: "var(--active-button-bg)",
							color: status.connected
								? "var(--attention-button-fg)"
								: "var(--window-fg-active)",
							transitionDuration: "var(--duration-universal)",
						}}
					>
						{busy
							? status.connected
								? "Disconnecting…"
								: "Waiting for your browser…"
							: status.connected
								? "Disconnect"
								: "Connect"}
					</button>
				)}
			</div>

			{!status.configured && (
				<p className="mt-3 text-[var(--window-sub-text-fg)]">
					This build was compiled without a Google client id, so Drive is
					unavailable. That is a packaging setting, not something to fix here.
				</p>
			)}

			{status.configured && !status.connected && (
				<ul
					className="mt-3 space-y-1 text-[var(--window-sub-text-fg)]"
					aria-label="What connecting allows"
				>
					{/* The scope in plain terms. `drive.file` is genuinely narrow and
					    saying so is the difference between informed consent and a
					    shrug, but it only counts if it is said before the click. */}
					<li>Vardo sees only the files you pick, or that it creates.</li>
					<li>The rest of your Drive stays invisible to it.</li>
					<li>You sign in in your normal browser, not inside this window.</li>
				</ul>
			)}

			{busy && !status.connected && (
				<p className="mt-3" style={{ color: "var(--window-active-text-fg)" }}>
					Finish signing in in your browser, then come back here.
				</p>
			)}

			{error && (
				<p className="mt-3" style={{ color: "var(--attention-button-fg)" }}>
					{error}
				</p>
			)}
		</section>
	);
}
