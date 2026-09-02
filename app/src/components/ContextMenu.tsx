import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * A right-click menu.
 *
 * There was none anywhere, which is the sort of absence that does not look like
 * anything until you try to rename a group and find there is no way to. Every
 * desktop application has this and people reach for it without thinking; a
 * program that ignores the right mouse button feels like a web page in a
 * window.
 *
 * Positioned against the viewport rather than the click, because a menu opened
 * near the bottom edge that renders downwards is a menu with items you cannot
 * reach. Measured after mount and moved if it would overflow.
 */
export interface MenuItem {
	label: string;
	onSelect: () => void;
	/** Rendered in the attention colour and separated from the rest. */
	destructive?: boolean;
}

interface Props {
	x: number;
	y: number;
	items: MenuItem[];
	onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: Props) {
	const ref = useRef<HTMLDivElement>(null);
	const [pos, setPos] = useState({ left: x, top: y });

	useLayoutEffect(() => {
		const el = ref.current;
		if (!el) return;
		const box = el.getBoundingClientRect();
		const margin = 8;
		setPos({
			left: Math.min(x, window.innerWidth - box.width - margin),
			top: Math.min(y, window.innerHeight - box.height - margin),
		});
	}, [x, y]);

	useEffect(() => {
		// Escape and any click elsewhere. `capture` so a click that would also
		// activate something underneath closes the menu first rather than doing
		// both, which is how you delete the wrong thing.
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") onClose();
		};
		const onDown = (e: MouseEvent) => {
			if (!ref.current?.contains(e.target as Node)) onClose();
		};
		window.addEventListener("keydown", onKey);
		window.addEventListener("mousedown", onDown, true);
		// A menu still open after the window scrolls is pointing at nothing.
		window.addEventListener("scroll", onClose, true);
		window.addEventListener("resize", onClose);
		return () => {
			window.removeEventListener("keydown", onKey);
			window.removeEventListener("mousedown", onDown, true);
			window.removeEventListener("scroll", onClose, true);
			window.removeEventListener("resize", onClose);
		};
	}, [onClose]);

	return (
		<div
			ref={ref}
			role="menu"
			className="fixed z-50 min-w-[180px] rounded-[6px] py-1"
			style={{
				left: pos.left,
				top: pos.top,
				background: "var(--menu-bg, var(--window-bg-over))",
				border: "1px solid var(--shadow-fg)",
				boxShadow: "0 6px 24px rgba(0,0,0,0.45)",
				animation: "slide-up var(--duration-slide-wrap) var(--ease-out-circ)",
			}}
		>
			{items.map((item, index) => (
				<div key={item.label}>
					{/* A rule above the destructive item, so the thing that cannot
					    be undone is not adjacent to the thing you meant to click. */}
					{item.destructive && index > 0 && (
						<div
							aria-hidden
							className="my-1"
							style={{ borderTop: "1px solid var(--shadow-fg)" }}
						/>
					)}
					<button
						type="button"
						role="menuitem"
						onClick={() => {
							item.onSelect();
							onClose();
						}}
						className="btn btn-quiet block w-full cursor-pointer px-3 py-1.5 text-left"
						style={{
							color: item.destructive
								? "var(--attention-button-fg)"
								: "var(--window-fg)",
						}}
					>
						{item.label}
					</button>
				</div>
			))}
		</div>
	);
}

/**
 * Tracks where a context menu should open.
 *
 * Kept here rather than in each caller so the "close it before opening another"
 * rule exists in one place. Two menus open at once is a state nobody designs
 * and everybody eventually produces.
 */
export function useContextMenu<T>() {
	const [state, setState] = useState<{
		x: number;
		y: number;
		target: T;
	} | null>(null);

	function open(e: React.MouseEvent, target: T) {
		e.preventDefault();
		e.stopPropagation();
		setState({ x: e.clientX, y: e.clientY, target });
	}

	return { state, open, close: () => setState(null) };
}
