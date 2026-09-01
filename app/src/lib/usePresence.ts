import { useEffect, useRef, useState } from "react";

/**
 * Keeps a component mounted long enough to animate away.
 *
 * Everything in this app animated in and nothing animated out, because React
 * unmounts on the same frame the condition goes false. A panel that slides in
 * over 240ms and then vanishes between two frames does not read as one object
 * leaving - it reads as the animation being decorative on the way in and the
 * program losing track of it on the way out.
 *
 * There is a CSS-only answer now - `@starting-style` with
 * `transition-behavior: allow-discrete` - and the browser this gets tested in
 * supports it. The browser it *ships* in is WebKitGTK, a different engine
 * entirely, and an exit animation that silently does nothing there while
 * looking right here is precisely the bug that survives to release. Thirty
 * lines that work on both is the cheaper trade.
 *
 * Returns `mounted` for whether to render at all, and `visible` for which
 * direction it is going, so a component can pick its entry or exit animation
 * without tracking any of this itself.
 */
export interface Presence {
	/** Render the element at all. Stays true through the exit animation. */
	mounted: boolean;
	/** False while leaving, so the caller can pick the exit animation. */
	visible: boolean;
}

export function usePresence(show: boolean, exitMs: number): Presence {
	const [mounted, setMounted] = useState(show);
	const timer = useRef<number | undefined>(undefined);

	useEffect(() => {
		window.clearTimeout(timer.current);

		if (show) {
			setMounted(true);
			return;
		}

		// Nothing to unmount, and more importantly nothing to schedule: without
		// this an already-hidden element would set a timer on every render.
		if (!mounted) return;

		timer.current = window.setTimeout(() => setMounted(false), exitMs);
		return () => window.clearTimeout(timer.current);
	}, [show, exitMs, mounted]);

	// Cleared on unmount as well, or a component removed by its own parent
	// mid-exit leaves a timer that fires into nothing.
	useEffect(() => () => window.clearTimeout(timer.current), []);

	return { mounted, visible: show };
}

/**
 * The animation names to use for a given presence.
 *
 * A helper rather than a convention, because the failure mode of a convention
 * is a component that animates in and cuts out, which is the exact thing this
 * file exists to stop.
 */
export function presenceStyle(
	presence: Presence,
	enter: string,
	exit: string,
	enterMs: string,
	exitMs: string,
): React.CSSProperties {
	return {
		animation: presence.visible
			? `${enter} ${enterMs} var(--ease-out-circ)`
			: `${exit} ${exitMs} var(--ease-out-circ) forwards`,
	};
}
