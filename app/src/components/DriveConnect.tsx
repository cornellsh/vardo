import { useCallback, useEffect, useState } from "react";

import { DriveLogo } from "./DriveLogo";
import { Notice, splitTechnical } from "./Notice";
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

					{/* One short line. The reasoning lives under "What this allows"
					    below, as separate short lines, rather than as a paragraph
					    that wraps into a block nobody reads. */}
					<p className="text-[var(--window-sub-text-fg)]">
						{status.connected
							? "Connected"
							: "Open and sync vaults kept in Drive"}
					</p>
				</div>

				{status.configured && (
					<button
						type="button"
						disabled={busy}
						onClick={() => void (status.connected ? disconnect() : connect())}
						className="btn btn-quiet shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold transition-colors disabled:opacity-45"
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
								? "Signing out…"
								: "Waiting…"
							: status.connected
								? "Sign out"
								: "Sign in"}
					</button>
				)}
			</div>

			{!status.configured && (
				<p className="mt-3 text-[var(--window-sub-text-fg)]">
					Unavailable: this build has no Google client id.
				</p>
			)}

			{status.configured && !status.connected && (
				<ul
					className="mt-3 space-y-1 text-[var(--window-sub-text-fg)]"
					aria-label="What connecting allows"
				>
					{/* The scope in plain terms, one short line each. `drive.file` is
					    genuinely narrow and saying so is the difference between
					    informed consent and a shrug, but it only counts if it is said
					    before the click and read rather than skimmed. */}
					<li>Only the files you pick</li>
					<li>The rest of your Drive stays hidden</li>
					<li>You sign in in your own browser</li>
				</ul>
			)}

			{busy && !status.connected && (
				<p className="mt-3" style={{ color: "var(--window-active-text-fg)" }}>
					Finish signing in in your browser.
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
