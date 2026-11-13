import { useCallback, useEffect, useRef, useState } from "react";

import { ipc } from "../ipc";
import type { EntrySummary } from "../ipc";
import { errorText } from "../ipc/error";

/** How many matches the window shows. More means scrolling, and this is for a glance. */
const SHOWN = 8;

/**
 * The quick-access window: type, press Enter, the password is on the
 * clipboard and the window is gone.
 *
 * It never sees a password. Copying goes through Rust, which clears the
 * clipboard afterwards, exactly as from the main window.
 */
export function Quick() {
	const [query, setQuery] = useState("");
	const [rows, setRows] = useState<EntrySummary[]>([]);
	const [selected, setSelected] = useState(0);
	const [locked, setLocked] = useState(false);
	const [problem, setProblem] = useState<string | null>(null);
	const input = useRef<HTMLInputElement>(null);

	const done = useCallback(async (revealMain: boolean) => {
		await (await ipc()).quickDone?.(revealMain);
	}, []);

	// Each opening starts clean: a search left from last time would put the
	// wrong login one Enter away.
	const reset = useCallback(async () => {
		setQuery("");
		setRows([]);
		setProblem(null);
		input.current?.focus();
		try {
			await (await ipc()).vaultInfo();
			setLocked(false);
		} catch {
			setLocked(true);
		}
	}, []);

	useEffect(() => {
		void reset();
		let dispose: (() => void) | undefined;
		void (async () => {
			dispose = await (await ipc()).onQuickOpen?.(() => void reset());
		})();
		return () => dispose?.();
	}, [reset]);

	// Clicking anywhere else dismisses it, like every launcher.
	useEffect(() => {
		const onBlur = () => void done(false);
		window.addEventListener("blur", onBlur);
		return () => window.removeEventListener("blur", onBlur);
	}, [done]);

	useEffect(() => {
		const text = query.trim();
		if (locked || !text) {
			setRows([]);
			return;
		}
		let current = true;
		void (async () => {
			try {
				const found = await (await ipc()).listEntries({ query: text });
				if (!current) return;
				setRows(found.slice(0, SHOWN));
				setSelected(0);
			} catch (err) {
				if (current) setProblem(errorText(err));
			}
		})();
		return () => {
			current = false;
		};
	}, [query, locked]);

	async function copy(row: EntrySummary, key: "Password" | "UserName") {
		try {
			await (await ipc()).copyField(row.uuid, key);
			await done(false);
		} catch (err) {
			setProblem(errorText(err));
		}
	}

	function onKeyDown(e: React.KeyboardEvent) {
		if (e.key === "Escape") {
			e.preventDefault();
			void done(false);
		} else if (e.key === "ArrowDown") {
			e.preventDefault();
			setSelected((i) => Math.min(i + 1, rows.length - 1));
		} else if (e.key === "ArrowUp") {
			e.preventDefault();
			setSelected((i) => Math.max(i - 1, 0));
		} else if (e.key === "Enter") {
			e.preventDefault();
			if (locked) return void done(true);
			const row = rows[selected];
			if (row) void copy(row, e.shiftKey ? "UserName" : "Password");
		}
	}

	return (
		<div
			className="flex h-full flex-col"
			style={{ background: "var(--window-bg)", color: "var(--window-fg)" }}
		>
			<input
				ref={input}
				value={query}
				onChange={(e) => setQuery(e.target.value)}
				onKeyDown={onKeyDown}
				placeholder={locked ? "Vardo is locked" : "Search logins"}
				aria-label="Search logins"
				aria-controls="quick-results"
				autoComplete="off"
				spellCheck={false}
				className="w-full px-4 py-3 text-[17px] outline-none"
				style={{
					background: "var(--window-bg)",
					borderBottom: "1px solid var(--shadow-fg)",
				}}
			/>
			<div id="quick-results" role="listbox" aria-label="Matching logins" className="min-h-0 flex-1 overflow-y-auto py-1">
				{locked ? (
					<p className="px-4 py-3 text-[var(--window-sub-text-fg)]">
						Press Enter to unlock it in the main window.
					</p>
				) : (
					rows.map((row, i) => (
						<button
							key={row.uuid}
							type="button"
							role="option"
							aria-selected={i === selected}
							tabIndex={-1}
							onMouseEnter={() => setSelected(i)}
							onClick={() => void copy(row, "Password")}
							className="flex w-full flex-col items-start px-4 py-2 text-left"
							style={{
								background: i === selected ? "var(--dialogs-bg-active)" : "transparent",
								color: i === selected ? "var(--window-fg-active)" : "inherit",
							}}
						>
							<span className="font-semibold">{row.title || row.urlHost || "Untitled"}</span>
							<span className="text-[13px] opacity-80">{row.username || "No username"}</span>
						</button>
					))
				)}
				{!locked && query.trim() && rows.length === 0 && !problem && (
					<p className="px-4 py-3 text-[var(--window-sub-text-fg)]">Nothing matches.</p>
				)}
				{problem && <p className="px-4 py-3 text-[var(--attention-button-fg)]">{problem}</p>}
			</div>
			<p
				className="px-4 py-2 text-[12px] text-[var(--window-sub-text-fg)]"
				style={{ borderTop: "1px solid var(--shadow-fg)" }}
			>
				Enter copies the password. Shift+Enter copies the username. Esc closes.
			</p>
		</div>
	);
}
