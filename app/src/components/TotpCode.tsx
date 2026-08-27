import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import type { TotpCode as Code, Uuid } from "../ipc";

interface Props {
	uuid: Uuid;
	onCopy: () => void;
}

/**
 * A one-time code with the remaining time drawn as a ring.
 *
 * The ring is the reason this is not just text. A six-digit code is useless
 * without knowing whether it is about to expire, and "23s" as a number makes
 * you read and compare rather than glance. Telegram uses the same radial
 * treatment for anything with a deadline, at `radialDuration: 350`.
 *
 * The code itself is mono with tabular figures, so the digits do not shift as
 * they change — the same functional reason the whole app uses `tnum` for
 * anything counting.
 */
export function TotpCode({ uuid, onCopy }: Props) {
	const [code, setCode] = useState<Code | null>(null);
	const [remaining, setRemaining] = useState(0);

	// Re-fetch when the window closes rather than polling every second: the
	// code only changes on a period boundary, and the countdown between them is
	// arithmetic we can do here.
	useEffect(() => {
		let cancelled = false;
		let timer: number | undefined;

		const refresh = async () => {
			try {
				const next = await (await ipc()).totp(uuid);
				if (cancelled || !next) return;
				setCode(next);
				setRemaining(next.secondsRemaining);
				timer = window.setTimeout(refresh, next.secondsRemaining * 1000);
			} catch {
				/* an entry without a seed simply has no code */
			}
		};
		void refresh();

		const tick = window.setInterval(() => {
			setRemaining((r) => (r > 0 ? r - 1 : 0));
		}, 1000);

		return () => {
			cancelled = true;
			window.clearTimeout(timer);
			window.clearInterval(tick);
		};
	}, [uuid]);

	if (!code) return null;

	const fraction = code.period > 0 ? remaining / code.period : 0;
	// 20px circle, 2px stroke.
	const radius = 9;
	const circumference = 2 * Math.PI * radius;
	// Below ten seconds the code is about to be useless; say so in colour
	// rather than making the user watch the number.
	const urgent = remaining <= 10;

	return (
		<div className="flex items-center gap-2">
			<button
				type="button"
				onClick={onCopy}
				title="Copy one-time code"
				className="secret cursor-pointer rounded-[var(--button-radius)] px-1 text-[15px] tracking-[0.08em] transition-colors"
				style={{
					color: urgent ? "var(--attention-button-fg)" : "var(--window-fg)",
					transitionDuration: "var(--duration-universal)",
				}}
			>
				{/* Grouped in threes, the way every authenticator shows them. */}
				{code.code.length === 6
					? `${code.code.slice(0, 3)} ${code.code.slice(3)}`
					: code.code}
			</button>

			<svg
				width="20"
				height="20"
				viewBox="0 0 20 20"
				aria-label={`${remaining} seconds remaining`}
				role="img"
			>
				<circle
					cx="10"
					cy="10"
					r={radius}
					fill="none"
					strokeWidth="2"
					stroke="var(--window-bg-over)"
				/>
				<circle
					cx="10"
					cy="10"
					r={radius}
					fill="none"
					strokeWidth="2"
					strokeLinecap="round"
					stroke={
						urgent ? "var(--attention-button-fg)" : "var(--window-bg-active)"
					}
					strokeDasharray={circumference}
					strokeDashoffset={circumference * (1 - fraction)}
					// Rotate so the ring drains from twelve o'clock.
					transform="rotate(-90 10 10)"
					style={{
						transition:
							"stroke-dashoffset 1s linear, stroke var(--duration-universal)",
					}}
				/>
			</svg>
		</div>
	);
}
