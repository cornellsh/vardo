import { useEffect, useRef, useState } from "react";

import { ipc } from "../ipc";

/** Requests in flight at once: quick enough for hundreds, gentle on each site. */
const PARALLEL = 4;

type Props = {
	/** Show what arrived so far. */
	onProgress: () => void;
	/** Store the icons in the vault file. */
	onFinished: () => Promise<void>;
};

/**
 * Fetching every missing site icon at once, only when asked.
 *
 * Unlocking never does this: it would send a request to every site in the
 * vault in one burst (`inv:no-phone-home`). Here the person sees how many sites
 * will be contacted before anything is sent, and can stop at any point.
 */
export function IconFetch({ onProgress, onFinished }: Props) {
	const [missing, setMissing] = useState<string[] | null>(null);
	const [done, setDone] = useState(0);
	const [found, setFound] = useState(0);
	const [running, setRunning] = useState(false);
	const [finished, setFinished] = useState(false);
	const stop = useRef(false);

	useEffect(() => {
		let gone = false;
		void (async () => {
			const list = await (await ipc()).entriesMissingIcons().catch(() => []);
			if (!gone) setMissing(list);
		})();
		return () => {
			gone = true;
			stop.current = true;
		};
	}, []);

	async function run(uuids: string[]) {
		stop.current = false;
		setDone(0);
		setFound(0);
		setRunning(true);
		setFinished(false);
		const api = await ipc();
		const queue = [...uuids];
		let arrived = 0;
		const worker = async () => {
			for (let uuid = queue.shift(); uuid && !stop.current; uuid = queue.shift()) {
				// A site that is down or has no icon is ordinary, not an error.
				const png = await api.fetchIcon(uuid).catch(() => null);
				setDone((n) => n + 1);
				if (png) {
					setFound((n) => n + 1);
					// Every tenth, not every one: each refresh reloads the whole list.
					if (++arrived % 10 === 0) onProgress();
				}
			}
		};
		await Promise.all(Array.from({ length: PARALLEL }, worker));
		await onFinished().catch(() => {});
		onProgress();
		setRunning(false);
		setFinished(true);
		setMissing(await api.entriesMissingIcons().catch(() => []));
	}

	if (!missing) return null;
	const total = missing.length;

	let status: string;
	if (running) status = `Getting icons, ${done} of ${total}…`;
	else if (finished)
		status =
			found === done
				? `Found ${found} ${found === 1 ? "icon" : "icons"}.`
				: `Found ${found} of ${done}. The other sites don't offer one.`;
	else if (total === 0) status = "Every entry with a web address has an icon.";
	else
		status = `${total} ${total === 1 ? "entry has" : "entries have"} no icon yet. Vardo will ask each site for its own icon, once. No other service is involved.`;

	return (
		<section
			className="rounded-[6px] p-4"
			style={{
				background: "var(--window-bg-over)",
				border: "1px solid var(--shadow-fg)",
			}}
		>
			<div className="flex items-start gap-3">
				<div className="min-w-0 flex-1">
					<h3 className="font-semibold text-[var(--window-bold-fg)] text-[15px]">
						Site icons
					</h3>
					<p className="text-[var(--window-sub-text-fg)]" aria-live="polite">
						{status}
					</p>
				</div>

				{(running || (total > 0 && !finished)) && (
					<button
						type="button"
						onClick={() => {
							if (running) stop.current = true;
							else void run(missing);
						}}
						className="btn btn-quiet shrink-0 cursor-pointer rounded-[var(--button-radius)] px-3 py-1.5 font-semibold transition-colors"
						style={{
							background: running ? "transparent" : "var(--active-button-bg)",
							color: "var(--window-fg-active)",
							transitionDuration: "var(--duration-universal)",
						}}
					>
						{running ? "Stop" : `Get ${total} ${total === 1 ? "icon" : "icons"}`}
					</button>
				)}
			</div>
		</section>
	);
}
