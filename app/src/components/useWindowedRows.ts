import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * Renders only the rows that are on screen.
 *
 * Measured before writing this, on a vault of twenty thousand entries: one
 * keystroke in the search box took 1754ms, and scrolling ran at 174ms a frame,
 * which is about six frames a second. Every row was in the DOM, so every
 * keystroke rebuilt twenty thousand of them.
 *
 * No dependency, because the hard part of virtualisation is variable row
 * heights and Telegram's list does not have them: `dialogsRowHeight` is a
 * constant 62px. With a fixed height the visible range is arithmetic, and a
 * library would be several hundred kilobytes to avoid four lines of division.
 *
 * The overscan exists so a fast scroll does not show blank space before React
 * catches up. Ten rows is roughly a screenful either side.
 */
const OVERSCAN = 10;

export interface Windowed {
	/** Attach to the scrolling element. */
	ref: React.RefObject<HTMLElement | null>;
	/** Index of the first row to render. */
	start: number;
	/** One past the last row to render. */
	end: number;
	/** Total height of all rows, so the scrollbar is the right size. */
	totalHeight: number;
	/** Where the rendered slice begins, for the spacer above it. */
	offsetTop: number;
	/** Bring a row into view, for keyboard navigation. */
	scrollToRow: (index: number) => void;
}

export function useWindowedRows(count: number, rowHeight: number): Windowed {
	const ref = useRef<HTMLElement | null>(null);
	const [scrollTop, setScrollTop] = useState(0);
	const [viewport, setViewport] = useState(0);

	// Measured rather than assumed: the list shares a column whose width is a
	// clamp of the window, and its height depends on whatever banners are
	// showing above it.
	useLayoutEffect(() => {
		const el = ref.current;
		if (!el) return;
		const measure = () => setViewport(el.clientHeight);
		measure();

		const observer = new ResizeObserver(measure);
		observer.observe(el);
		return () => observer.disconnect();
	}, []);

	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		// Passive: this listener never calls preventDefault, and saying so lets
		// the browser scroll without waiting to find out.
		const onScroll = () => setScrollTop(el.scrollTop);
		el.addEventListener("scroll", onScroll, { passive: true });
		return () => el.removeEventListener("scroll", onScroll);
	}, []);

	const visible = Math.ceil(viewport / rowHeight) + OVERSCAN * 2;
	const start = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
	const end = Math.min(count, start + visible);

	const scrollToRow = useCallback(
		(index: number) => {
			const el = ref.current;
			if (!el) return;
			const top = index * rowHeight;
			const bottom = top + rowHeight;
			// Only move when the row is actually outside, so arrowing through
			// the middle of a screenful does not drag the list around.
			if (top < el.scrollTop) {
				el.scrollTop = top;
			} else if (bottom > el.scrollTop + el.clientHeight) {
				el.scrollTop = bottom - el.clientHeight;
			}
		},
		[rowHeight],
	);

	return {
		ref,
		start,
		end,
		totalHeight: count * rowHeight,
		offsetTop: start * rowHeight,
		scrollToRow,
	};
}
