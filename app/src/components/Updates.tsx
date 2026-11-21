import { useEffect, useState } from "react";

import { Notice } from "./Notice";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { UpdateChecks, UpdateStatus } from "../ipc";

/**
 * Updating Vardo itself (`dec:auto-update`).
 *
 * Rust checks, downloads and verifies; this only asks the person once whether
 * to check, and offers the restart when a verified update is waiting.
 */
function useUpdates() {
	const [status, setStatus] = useState<UpdateStatus | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		let dispose: (() => void) | undefined;
		let live = true;
		void (async () => {
			const api = await ipc();
			const first = await api.updateStatus();
			if (live) setStatus(first);
			dispose = await api.onUpdate((next) => setStatus(next));
			if (!live) dispose();
		})();
		return () => {
			live = false;
			dispose?.();
		};
	}, []);

	async function run(action: () => Promise<UpdateStatus | void>) {
		setError(null);
		try {
			const next = await action();
			if (next) setStatus(next);
		} catch (err) {
			setError(errorText(err));
		}
	}

	return {
		status,
		error,
		setChecks: (checks: UpdateChecks) =>
			run(async () => (await ipc()).setUpdateChecks(checks)),
		checkNow: () => run(async () => (await ipc()).checkForUpdate()),
		install: () => run(async () => (await ipc()).installUpdate()),
	};
}

/** For the unlock screen: the one-time question, or the waiting update. */
export function UpdatePrompt() {
	const { status, error, setChecks, install } = useUpdates();
	if (!status) return null;

	if (status.state.kind === "ready") {
		return (
			<div className="mt-6">
				<Notice
					tone="info"
					title={`Vardo ${status.state.version} is ready`}
					{...(error ? { detail: error } : {})}
					action={{ label: "Restart to update", onClick: () => void install() }}
				/>
			</div>
		);
	}
	if (status.checks !== "unasked") return null;

	return (
		<section aria-label="Updates" className="mt-6 text-center">
			<p className="text-[var(--window-bold-fg)]">Check for updates once a day?</p>
			<p className="mt-0.5 text-[var(--window-sub-text-fg)]">
				Vardo asks GitHub if there is a newer version. Nothing about your vaults is sent.
			</p>
			<div className="mt-2 flex justify-center gap-2">
				<button
					type="button"
					onClick={() => void setChecks("on")}
					className="btn btn-quiet cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold"
					style={{ color: "var(--window-active-text-fg)" }}
				>
					Check daily
				</button>
				<button
					type="button"
					onClick={() => void setChecks("off")}
					className="btn btn-quiet cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Not now
				</button>
			</div>
			{error && (
				<div className="mt-3 text-left">
					<Notice title={error} />
				</div>
			)}
		</section>
	);
}

function stateText(status: UpdateStatus): string {
	const s = status.state;
	switch (s.kind) {
		case "idle":
			return status.checks === "on" ? "Checks once a day" : "Not checking";
		case "checking":
			return "Checking…";
		case "upToDate":
			return "Up to date";
		case "downloading":
			return `Downloading ${s.version}…`;
		case "ready":
			return `Vardo ${s.version} is ready`;
		case "failed":
			return "Could not check for updates";
	}
}

/** For Settings: the version, the daily check, and the restart. */
export function UpdateSettings() {
	const { status, error, setChecks, checkNow, install } = useUpdates();
	if (!status) return null;
	const s = status.state;
	const busy = s.kind === "checking" || s.kind === "downloading";

	return (
		<section
			className="rounded-[6px] p-4"
			style={{ background: "var(--window-bg-over)", border: "1px solid var(--shadow-fg)" }}
		>
			<div className="flex items-start gap-3">
				<div className="min-w-0 flex-1">
					<h3 className="font-semibold text-[var(--window-bold-fg)] text-[15px]">
						Vardo {status.current}
					</h3>
					<p aria-live="polite" className="text-[var(--window-sub-text-fg)]">
						{stateText(status)}
					</p>
				</div>
				{s.kind === "ready" ? (
					<button
						type="button"
						onClick={() => void install()}
						className="btn btn-quiet shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold"
						style={{ background: "var(--active-button-bg)", color: "var(--window-fg-active)" }}
					>
						Restart to update
					</button>
				) : (
					<button
						type="button"
						disabled={busy}
						onClick={() => void checkNow()}
						className="btn btn-quiet shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold disabled:opacity-45"
						style={{ color: "var(--window-fg-active)" }}
					>
						Check now
					</button>
				)}
			</div>

			<label className="mt-3 flex cursor-pointer items-center gap-2">
				<input
					type="checkbox"
					checked={status.checks === "on"}
					onChange={(e) => void setChecks(e.target.checked ? "on" : "off")}
				/>
				<span>Check for updates once a day</span>
			</label>

			{s.kind === "failed" && (
				<div className="mt-3">
					<Notice tone="warning" title="Could not check for updates" technical={s.message} />
				</div>
			)}
			{error && (
				<div className="mt-3">
					<Notice title={error} />
				</div>
			)}
		</section>
	);
}
