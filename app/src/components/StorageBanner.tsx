import { useState } from "react";

import type { StorageWarning } from "../ipc";

interface Props {
	warning: StorageWarning;
}

/**
 * Warns that the vault is somewhere that can corrupt it.
 *
 * Deliberately dismissible but not permanently silenced: this is advisory, and
 * a warning that cannot be dismissed becomes noise the user learns to ignore —
 * but one that is silenced forever stops being a warning at all. It comes back
 * next unlock, which is also when it is cheapest to act on.
 *
 * The copy names the specific hazard rather than saying "this may be unsafe".
 * A vague warning about a real risk is indistinguishable from superstition, and
 * users are right to ignore those.
 */
export function StorageBanner({ warning }: Props) {
	const [open, setOpen] = useState(false);
	const [dismissed, setDismissed] = useState(false);

	if (dismissed) return null;

	return (
		<div
			className="shrink-0 px-4 py-2"
			style={{
				background: "var(--window-bg-over)",
				borderBottom: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="flex items-start gap-2">
				<span
					aria-hidden
					className="mt-1 h-2 w-2 shrink-0 rounded-full"
					style={{ background: "var(--attention-button-fg)" }}
				/>
				<div className="min-w-0 flex-1">
					{/* The headline and the remedy are each one short line. The
					    reasoning is real and worth keeping, but it belongs behind
					    the disclosure rather than across four lines of banner. */}
					<div className="text-[var(--window-fg)]">
						{warning.explanation}{" "}
						<button
							type="button"
							onClick={() => setOpen((o) => !o)}
							className="cursor-pointer"
							style={{ color: "var(--window-active-text-fg)" }}
							aria-expanded={open}
						>
							{open ? "Less" : "Why"}
						</button>
					</div>

					{open && (
						<div className="mt-1.5 space-y-1.5 text-[var(--window-sub-text-fg)]">
							<p>{warning.why}</p>
							<p style={{ color: "var(--window-fg)" }}>{warning.remedy}</p>
						</div>
					)}
				</div>

				<button
					type="button"
					onClick={() => setDismissed(true)}
					aria-label="Dismiss for now"
					title="Dismiss for now — returns next unlock"
					className="shrink-0 cursor-pointer rounded-[6px] px-2 py-0.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					×
				</button>
			</div>
		</div>
	);
}
