import type { StorageWarning } from "../ipc";

interface Props {
	warning: StorageWarning;
}

/**
 * Where the vault is kept, and why that might matter.
 *
 * This used to be a banner across the top of the window that pushed the whole
 * app down on every unlock, and expanded in place so reading it moved
 * everything again. Three things were wrong with that.
 *
 * It interrupted at the worst moment. Someone has just typed their password to
 * look something up; a paragraph about filesystem semantics is not what they
 * came for, and a warning shown at a moment nobody can act on is one they learn
 * to dismiss without reading.
 *
 * It offered a button that could not do what it said. "Connect Google Drive"
 * does not move a vault into Drive - the account and the file are separate
 * things, and connecting one changes nothing about a file sitting on a mount.
 * A control that promises a fix it cannot perform is worse than no control.
 *
 * And it was in the wrong place. The rail already says where the vault lives,
 * which is the fact this is about, so the marker belongs there and the
 * explanation belongs here in settings, where there is room to read it.
 */
export function StorageNote({ warning }: Props) {
	return (
		<section
			className="rounded-[6px] p-4"
			style={{
				background: "var(--window-bg-over)",
				border: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="flex items-start gap-3">
				<svg
					width="18"
					height="18"
					viewBox="0 0 24 24"
					fill="none"
					stroke="#e8a33d"
					strokeWidth="2"
					strokeLinecap="round"
					aria-hidden
					className="mt-0.5 shrink-0"
				>
					<path d="M12 4l9 16H3z" />
					<path d="M12 10v4M12 17.5v.01" />
				</svg>

				<div className="min-w-0" style={{ maxWidth: "60ch" }}>
					<h3 className="font-semibold text-[15px] text-[var(--window-bold-fg)]">
						{warning.explanation}
					</h3>
					<p className="mt-1 text-[var(--window-sub-text-fg)]">{warning.why}</p>

					{/* Stated as what to do, without a button pretending to do it.
					    Moving a vault into Drive means opening it from there, which
					    is a deliberate act on the unlock screen and not something
					    that should happen behind a control in settings. */}
					<p className="mt-2 text-[var(--window-fg)]">{warning.remedy}</p>
					{warning.providerHint === "google-drive" && (
						<p className="mt-1 text-[var(--window-sub-text-fg)]">
							Lock the vault, then use Choose from Drive.
						</p>
					)}
				</div>
			</div>
		</section>
	);
}
