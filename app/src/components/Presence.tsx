import type { ReactNode } from "react";

import { usePresence } from "../lib/usePresence";

/**
 * Shows its children with an entrance, and keeps them long enough to leave.
 *
 * The alternative was every caller repeating the same four lines, and the
 * failure mode of that is the one this exists to fix: someone adds a panel,
 * gives it an entrance because that is the fun half, and it disappears between
 * two frames on the way out. One wrapper means the exit is not something you
 * have to remember.
 *
 * Durations default to Telegram's `slideWrapDuration`, and the exit is
 * deliberately allowed to differ: arriving is worth watching, leaving should
 * get out of the way.
 */
interface Props {
	show: boolean;
	children: ReactNode;
	enter?: string;
	exit?: string;
	/** Milliseconds. Must match the exit animation, or it is cut off. */
	exitMs?: number;
	enterDuration?: string;
	exitDuration?: string;
	className?: string;
}

export function Presence({
	show,
	children,
	enter = "slide-up",
	exit = "collapse-out",
	exitMs = 150,
	enterDuration = "var(--duration-slide-wrap)",
	exitDuration = "var(--duration-slide-wrap)",
	className,
}: Props) {
	const presence = usePresence(show, exitMs);
	if (!presence.mounted) return null;

	return (
		<div
			{...(className ? { className } : {})}
			style={{
				animation: presence.visible
					? `${enter} ${enterDuration} var(--ease-out-circ)`
					: `${exit} ${exitDuration} var(--ease-out-circ) forwards`,
				// Off during the exit, so a control cannot be clicked while it is
				// fading away and half gone.
				pointerEvents: presence.visible ? undefined : "none",
			}}
		>
			{children}
		</div>
	);
}
