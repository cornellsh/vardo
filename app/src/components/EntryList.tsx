import { useEffect, useMemo, useRef } from "react";

import type { EntrySummary, Uuid } from "../ipc";
import { useRipple } from "./Ripple";
import { identityOf, initialOf, tintFor } from "../lib/identity";
import { useWindowedRows } from "./useWindowedRows";

interface Props {
	rows: EntrySummary[];
	loading: boolean;
	error: string | null;
	query: string;
	onQuery: (q: string) => void;
	sort: "name" | "recent";
	onSort: (s: "name" | "recent") => void;
	/**
	 * Take the whole width rather than a fixed column.
	 *
	 * True when the list is the only pane showing, which happens in a window
	 * too narrow for two. Holding the column width there would leave dead space
	 * beside a list that has nothing to sit next to.
	 */
	fill?: boolean | undefined;
	/** Opens the group drawer. Absent when the rail is a permanent column. */
	onOpenGroups?: (() => void) | undefined;
	selected: Uuid | null;
	onSelect: (uuid: Uuid) => void;
	/** Absent on a read-only database, which hides the button entirely rather
	 * than showing one that always fails. */
	onAdd?: () => void;
}

/** Telegram's chat list. `dialogsRowHeight: 62px`, an avatar and two text lines. */
export function EntryList({
	rows,
	loading,
	error,
	query,
	onQuery,
	sort,
	onSort,
	selected,
	onSelect,
	onAdd,
	fill,
	onOpenGroups,
}: Props) {
	// Fixed row height is what makes this arithmetic rather than a library.
	const windowed = useWindowedRows(rows.length, ROW_HEIGHT);
	const searchRef = useRef<HTMLInputElement>(null);

	// A vault holds many entries per service, so "Airbnb" ten times over is
	// normal rather than exceptional — and a row showing only a title and a
	// username is then genuinely ambiguous. Telegram never has this problem
	// because chat names are close to unique. Rows whose title is repeated in the
	// current list get the host appended; the rest stay clean.
	const duplicated = useMemo(() => {
		const seen = new Set<string>();
		const dupes = new Set<string>();
		for (const r of rows) {
			if (seen.has(r.title)) dupes.add(r.title);
			else seen.add(r.title);
		}
		return dupes;
	}, [rows]);

	useEffect(() => {
		function onKey(e: KeyboardEvent) {
			if (e.key === "f" && (e.ctrlKey || e.metaKey)) {
				e.preventDefault();
				searchRef.current?.focus();
				searchRef.current?.select();
			}
		}
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, []);

	function move(delta: number) {
		if (rows.length === 0) return;
		const at = rows.findIndex((r) => r.uuid === selected);
		const next =
			at === -1 ? 0 : Math.min(rows.length - 1, Math.max(0, at + delta));
		const row = rows[next];
		if (!row) return;
		onSelect(row.uuid);
		// By index, not by finding the element: the row being moved to is very
		// often not rendered yet, which is the whole point of windowing, and
		// `scrollIntoView` on a node that does not exist silently does nothing.
		windowed.scrollToRow(next);
	}

	function onListKey(e: React.KeyboardEvent) {
		if (e.key === "ArrowDown") {
			e.preventDefault();
			move(1);
		} else if (e.key === "ArrowUp") {
			e.preventDefault();
			move(-1);
		}
	}

	return (
		<section
			aria-label="Entries"
			className="flex shrink-0 flex-col"
			style={{
				// Telegram: clamp(windowWidth * 5/14, min, max), from mainwidget.h.
				// Dropped when this is the only pane, because a fixed column in a
				// window with nothing beside it is just a margin.
				...(fill
					? { flex: "1 1 auto", minWidth: 0 }
					: {
							width:
								"clamp(var(--column-min-main), calc(100vw * 5 / 14), var(--column-max-left))",
						}),
				background: "var(--dialogs-bg)",
				borderRight: "1px solid var(--shadow-fg)",
			}}
		>
			<div
				className="flex items-center px-2"
				style={{ height: "54px", borderBottom: "1px solid var(--shadow-fg)" }}
			>
				{/* Only when the rail is not a column of its own. Otherwise the
				    groups are already on screen and this would be a button that
				    opens what you are looking at. */}
				{onOpenGroups && (
					<button
						type="button"
						onClick={onOpenGroups}
						aria-label="Groups"
						title="Groups"
						className="btn btn-quiet mr-1 flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full"
						style={{ color: "var(--window-sub-text-fg)" }}
					>
						<svg
							width="18"
							height="18"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							strokeWidth="2"
							strokeLinecap="round"
							aria-hidden
						>
							<path d="M4 7h16M4 12h16M4 17h16" />
						</svg>
					</button>
				)}
				<label htmlFor="search" className="sr-only">
					Search entries
				</label>
				<input
					ref={searchRef}
					id="search"
					type="search"
					value={query}
					placeholder="Search  (Ctrl+F)"
					spellCheck={false}
					autoComplete="off"
					onChange={(e) => onQuery(e.target.value)}
					onKeyDown={(e) => {
						if (e.key === "ArrowDown") {
							e.preventDefault();
							move(1);
						}
					}}
					className="w-full rounded-[16px] border-0 px-3 outline-none"
					style={{
						height: "32px",
						background: "var(--window-bg-over)",
						color: "var(--window-fg)",
					}}
				/>
				{/* Ordering, as a two-state toggle rather than a menu.
				    There are only two useful orders and a dropdown for two
				    options costs a click to discover what it even contains. */}
				<button
					type="button"
					onClick={() => onSort(sort === "name" ? "recent" : "name")}
					title={
						sort === "name"
							? "Sorted by name. Switch to most recent."
							: "Sorted by most recent. Switch to name."
					}
					aria-label={
						sort === "name" ? "Sorted by name" : "Sorted by most recent"
					}
					className="btn btn-quiet ml-1 flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					{sort === "name" ? (
						<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden>
							<path d="M6 4v16M6 20l-3-3M6 20l3-3" />
							<path d="M13 6h8M13 12h6M13 18h4" />
						</svg>
					) : (
						<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden>
							<circle cx="12" cy="12" r="8" />
							<path d="M12 8v4l3 2" />
						</svg>
					)}
				</button>
				{onAdd && (
					<button
						type="button"
						onClick={onAdd}
						title="New entry (Ctrl+N)"
						aria-label="New entry"
						className="btn btn-accent ml-1 flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full"
						style={{ color: "var(--window-fg-active)" }}
					>
						<svg
							width="16"
							height="16"
							viewBox="0 0 24 24"
							fill="none"
							stroke="currentColor"
							strokeWidth="2"
							strokeLinecap="round"
							aria-hidden
						>
							<path d="M12 5v14M5 12h14" />
						</svg>
					</button>
				)}
			</div>

			<ul
				ref={windowed.ref as React.RefObject<HTMLUListElement>}
				// biome-ignore lint/a11y/noNoninteractiveTabindex: the list is the
				// keyboard target for arrow navigation between rows.
				tabIndex={0}
				onKeyDown={onListKey}
				className="flex-1 overflow-y-auto outline-none"
			>
				{error && (
					<li
						className="px-4 py-6"
						style={{ color: "var(--attention-button-fg)" }}
					>
						{error}
					</li>
				)}
				{!error && loading && rows.length === 0 && (
					<li className="px-4 py-6 text-[var(--window-sub-text-fg)]">
						Loading…
					</li>
				)}
				{!error && !loading && rows.length === 0 && (
					<li className="px-4 py-6 text-[var(--window-sub-text-fg)]">
						{query ? (
							<>
								Nothing matches <span className="font-semibold">{query}</span>.
							</>
						) : (
							"This group is empty."
						)}
					</li>
				)}
				{/* Only the visible slice is built, held in place by a spacer of
				    the full list height so the scrollbar still tells the truth
				    about how much there is. */}
				{rows.length > 0 && (
					<li style={{ height: windowed.totalHeight, position: "relative" }}>
						<ul
							style={{
								position: "absolute",
								top: windowed.offsetTop,
								left: 0,
								right: 0,
							}}
						>
							{rows.slice(windowed.start, windowed.end).map((row) => (
								<Row
									key={row.uuid}
									row={row}
									active={row.uuid === selected}
									ambiguous={duplicated.has(row.title)}
									onClick={() => onSelect(row.uuid)}
								/>
							))}
						</ul>
					</li>
				)}
			</ul>
		</section>
	);
}

/** Telegram's `dialogsRowHeight`. Must match the row's own height exactly, or
 * the windowing drifts from what is painted. */
const ROW_HEIGHT = 62;

/** Real vaults contain untitled entries; they are not an error. */
const UNTITLED = "Untitled";

/**
 * A compact age for the right edge of a row.
 *
 * The weeks step is not decoration. Without it anything between eight and
 * twenty-nine days old divided to zero months and rendered as "0mo", which is
 * both wrong and unmistakably a bug to anyone who notices it.
 */
function relative(ms: number): string {
	const days = Math.floor((Date.now() - ms) / 86_400_000);
	if (days <= 0) return "today";
	if (days === 1) return "1d";
	if (days < 7) return `${days}d`;
	if (days < 30) return `${Math.floor(days / 7)}w`;
	if (days < 365) return `${Math.max(1, Math.floor(days / 30))}mo`;
	return `${Math.max(1, Math.floor(days / 365))}y`;
}

function Row({
	row,
	active,
	ambiguous,
	onClick,
}: {
	row: EntrySummary;
	active: boolean;
	ambiguous: boolean;
	onClick: () => void;
}) {
	const ripple = useRipple();
	const name = active ? "var(--window-fg-active)" : "var(--dialogs-name-fg)";
	const sub = active ? "var(--window-fg-active)" : "var(--dialogs-text-fg)";

	return (
		<li data-uuid={row.uuid}>
			<button
				type="button"
				onClick={onClick}
				aria-current={active ? "true" : undefined}
				{...ripple.handlers}
				// `row`, not a button style. A row is a place you are, not a
				// control you press, and the difference matters for the selected
				// one: hovering it must not repaint it, or the row you are looking
				// at appears to be about to change into something else.
				className="row relative flex w-full cursor-pointer items-center gap-3 overflow-hidden text-left"
				style={{
					height: "var(--dialogs-row-height)",
					padding: "0 var(--dialogs-padding-x)",
					background: active ? "var(--dialogs-bg-active)" : "transparent",
				}}
			>
				{ripple.layer}
				{/* `relative` on the content, so it paints above the absolutely
				    positioned ripple layer rather than under it. */}
				<span
					aria-hidden
					className="relative flex shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold"
					style={{
						width: "var(--dialogs-photo-size)",
						height: "var(--dialogs-photo-size)",
						// Tinted by what the entry is *for*, not by its UUID. Keying on
						// the UUID gave eight Airbnb rows eight different colours,
						// which made the list read as confetti instead of as a list.
						background: active
							? "rgba(255,255,255,0.22)"
							: tintFor(identityOf({ title: row.title, url: row.urlHost })),
						color: "#ffffff",
						fontSize: "17px",
					}}
				>
					{row.iconPng ? (
						// The vault's own icon, which for a KeePassXC user is often a
						// real site favicon it downloaded. Shown at 76% so a square
						// logo does not touch the edge of the circle.
						<img
							src={row.iconPng}
							alt=""
							style={{ width: "76%", height: "76%", objectFit: "contain" }}
						/>
					) : (
						initialOf(row.title)
					)}
				</span>

				<span className="relative flex min-w-0 flex-1 flex-col gap-[3px]">
					<span className="flex items-baseline gap-2">
						<span
							className="min-w-0 flex-1 truncate font-semibold"
							style={{
								color: name,
								fontStyle: row.title ? undefined : "italic",
							}}
						>
							{row.title || UNTITLED}
						</span>
						{row.hasTotp && (
							<span
								aria-label="has a one-time code"
								title="One-time code"
								style={{ color: sub }}
							>
								⧗
							</span>
						)}
						{row.hasAttachments && (
							<span
								aria-label="has attachments"
								title="Attachments"
								style={{ color: sub }}
							>
								⇲
							</span>
						)}
						<span
							className="shrink-0 tabular-nums"
							style={{ color: sub, fontSize: "12px" }}
						>
							{relative(row.modified)}
						</span>
					</span>
					<span className="truncate" style={{ color: sub }}>
						{/* An entry with no username is common and not an error, so the
                host stands in rather than leaving the line blank. */}
						{row.username || row.urlHost || "—"}
						{ambiguous && row.username && row.urlHost && (
							<span style={{ opacity: 0.75 }}> at {row.urlHost}</span>
						)}
					</span>
				</span>
			</button>
		</li>
	);
}
