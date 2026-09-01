import { useEffect, useState } from "react";

/**
 * How many of the three panes fit.
 *
 * Measured before choosing the numbers: the rail is a fixed 260, the list will
 * not go below 380, and the detail pane not below 292, so three panes need
 * 932px. Below that the app did not adapt - it overflowed. At 860 it was cut
 * off by 72 pixels, at 600 by 332, with the detail pane simply off the edge of
 * the window and no scrollbar to reach it.
 *
 * That is not an exotic case. Snapping a window to half a 1920 screen gives
 * 960, which squeaked through with 8 pixels to spare; half of a 1440 screen
 * gives 720, which did not.
 *
 * The thresholds sit above the arithmetic minimums rather than on them. A pane
 * at exactly its minimum is technically present and practically unusable, and
 * the point is to drop a pane before that happens rather than after.
 */
export type LayoutMode = "three" | "two" | "one";

/** rail 260 + list 380 + detail 292 = 932, plus room to not feel crushed. */
const THREE_PANE = 1000;

/** list 380 + detail 292 = 672, again with headroom. */
const TWO_PANE = 700;

export function useLayoutMode(): LayoutMode {
	const [mode, setMode] = useState<LayoutMode>(() =>
		modeFor(window.innerWidth),
	);

	useEffect(() => {
		// `resize` rather than a media query list, because there are two
		// thresholds and one listener reading the width is easier to reason
		// about than two query objects that can disagree mid-drag.
		const onResize = () => setMode(modeFor(window.innerWidth));
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, []);

	return mode;
}

function modeFor(width: number): LayoutMode {
	if (width >= THREE_PANE) return "three";
	if (width >= TWO_PANE) return "two";
	return "one";
}
