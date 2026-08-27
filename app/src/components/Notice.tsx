/**
 * A failure, presented so it can be acted on.
 *
 * The version this replaces was a paragraph of red text with a link under it.
 * Three problems: red for everything meant nothing was ranked, the technical
 * detail sat inline competing with the sentence that mattered, and there was no
 * visual anchor so it read as a wall.
 *
 * So: one line saying what happened, an optional line saying what to do, the
 * raw detail folded away for whoever needs it, and colour used to distinguish
 * "this failed" from "this needs your attention" rather than as decoration.
 */

type Tone = "error" | "warning" | "info";

interface Props {
	tone?: Tone;
	title: string;
	/** What to do about it. Omitted when there is nothing useful to say. */
	detail?: string;
	/** Raw text for diagnosis, hidden behind a disclosure. */
	technical?: string;
	action?: { label: string; onClick: () => void };
}

const TONES: Record<Tone, { fg: string; bg: string }> = {
	error: { fg: "var(--attention-button-fg)", bg: "rgba(226, 91, 91, 0.10)" },
	warning: { fg: "#e8a33d", bg: "rgba(232, 163, 61, 0.10)" },
	info: { fg: "var(--window-active-text-fg)", bg: "var(--window-bg-over)" },
};

export function Notice({
	tone = "error",
	title,
	detail,
	technical,
	action,
}: Props) {
	const { fg, bg } = TONES[tone];

	return (
		<div
			role={tone === "error" ? "alert" : "status"}
			className="rounded-[6px] p-3"
			style={{
				background: bg,
				// A hairline in the tone colour rather than tinting all the text.
				// Red body text is hard to read and stops ranking anything once
				// every word is red.
				borderLeft: `2px solid ${fg}`,
				animation: "slide-up var(--duration-slide-wrap) var(--ease-out-circ)",
			}}
		>
			<p className="font-semibold" style={{ color: fg }}>
				{title}
			</p>

			{detail && (
				<p className="mt-0.5" style={{ color: "var(--window-fg)" }}>
					{detail}
				</p>
			)}

			{technical && (
				<details className="mt-1.5">
					<summary
						className="cursor-pointer"
						style={{ color: "var(--window-sub-text-fg)", fontSize: "12px" }}
					>
						Details
					</summary>
					<p
						className="selectable mt-1 break-words"
						style={{
							color: "var(--window-sub-text-fg)",
							fontFamily: "var(--font-mono-stack)",
							fontSize: "12px",
						}}
					>
						{technical}
					</p>
				</details>
			)}

			{action && (
				<button
					type="button"
					onClick={action.onClick}
					className="mt-2 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold transition-colors"
					style={{
						background: "var(--active-button-bg)",
						color: "var(--window-fg-active)",
						transitionDuration: "var(--duration-universal)",
					}}
				>
					{action.label}
				</button>
			)}
		</div>
	);
}

/**
 * Splits a message into the sentence to show and the parenthetical to fold.
 *
 * Core errors carry their technical cause in trailing parentheses, which is the
 * right place for it in a log and the wrong place in a dialog. Rather than
 * strip it - it is the only thing that makes a bug report useful - it moves
 * behind the disclosure.
 */
export function splitTechnical(message: string): {
	message: string;
	technical?: string;
} {
	const match = message.match(/^(.*?)\s*\(([^()]*)\)\s*$/s);
	if (!match || !match[1] || !match[2]) return { message };
	return { message: match[1], technical: match[2] };
}
