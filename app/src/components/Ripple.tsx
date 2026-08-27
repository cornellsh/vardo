import { useCallback, useRef, useState } from "react";

/**
 * Telegram's press ripple.
 *
 * The most recognisable interaction in the app and, until now, the one piece of
 * the port that was missing entirely — `windowBgRipple` was carried over as a
 * colour and never drawn.
 *
 * Faithful to `defaultRippleAnimation` in `lib_ui/ui/widgets/widgets.style`:
 * the circle expands for 650ms and fades out over 200ms. Those are very
 * different numbers, and the asymmetry is the whole effect. The circle keeps
 * growing for as long as the pointer is held, so a long press feels heavier
 * than a tap, and then it leaves quickly so it never delays the result.
 *
 * Unlike Material's ripple this one is **not** centred and does not bounce: it
 * starts exactly where the pointer landed and grows to cover the element.
 */

interface Ripple {
	id: number;
	x: number;
	y: number;
	size: number;
	/** Set on pointer release, which starts the fade. */
	releasedAt: number | null;
}

export function useRipple() {
	const [ripples, setRipples] = useState<Ripple[]>([]);
	const next = useRef(0);

	const start = useCallback((e: React.PointerEvent<HTMLElement>) => {
		// Only the primary button; a right-click opens a menu and should not
		// look like a press.
		if (e.button !== 0) return;

		const el = e.currentTarget;
		const rect = el.getBoundingClientRect();
		const x = e.clientX - rect.left;
		const y = e.clientY - rect.top;

		// Reach the furthest corner, so the circle always covers the element
		// however far off-centre the press was.
		const size =
			2 *
			Math.max(
				Math.hypot(x, y),
				Math.hypot(rect.width - x, y),
				Math.hypot(x, rect.height - y),
				Math.hypot(rect.width - x, rect.height - y),
			);

		const id = next.current++;
		setRipples((rs) => [...rs, { id, x, y, size, releasedAt: null }]);
	}, []);

	const release = useCallback(() => {
		setRipples((rs) =>
			rs.map((r) => (r.releasedAt === null ? { ...r, releasedAt: Date.now() } : r)),
		);
	}, []);

	const remove = useCallback((id: number) => {
		setRipples((rs) => rs.filter((r) => r.id !== id));
	}, []);

	/** Spread onto the element that should ripple. It must be `position: relative`. */
	const handlers = {
		onPointerDown: start,
		onPointerUp: release,
		// A pointer that leaves mid-press still has to end the ripple, or it
		// stays on screen forever.
		onPointerLeave: release,
		onPointerCancel: release,
	};

	const layer = (
		<span
			aria-hidden
			className="pointer-events-none absolute inset-0 overflow-hidden"
			style={{ borderRadius: "inherit" }}
		>
			{ripples.map((r) => (
				<span
					key={r.id}
					onAnimationEnd={(e) => {
						// Only remove once the *fade* has finished; the expand
						// animation also fires this event.
						if (e.animationName.includes("ripple-out")) remove(r.id);
					}}
					style={{
						position: "absolute",
						left: r.x - r.size / 2,
						top: r.y - r.size / 2,
						width: r.size,
						height: r.size,
						borderRadius: "50%",
						background: "var(--window-bg-ripple)",
						transform: "scale(0)",
						animation:
							r.releasedAt === null
								? "ripple-in var(--ripple-show) var(--ease-out-quint) forwards"
								: "ripple-in var(--ripple-show) var(--ease-out-quint) forwards, ripple-out var(--ripple-hide) linear forwards",
					}}
				/>
			))}
		</span>
	);

	return { handlers, layer };
}
