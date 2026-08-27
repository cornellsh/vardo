import { useEffect, useMemo, useRef } from "react";

import type { EntrySummary, Uuid } from "../ipc";

interface Props {
	rows: EntrySummary[];
	loading: boolean;
	error: string | null;
	query: string;
	onQuery: (q: string) => void;
	selected: Uuid | null;
	onSelect: (uuid: Uuid) => void;
}

/** Telegram's chat list. `dialogsRowHeight: 62px`, an avatar and two text lines. */
export function EntryList({
	rows,
	loading,
	error,
	query,
	onQuery,
	selected,
	onSelect,
}: Props) {
	const listRef = useRef<HTMLUListElement>(null);
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
		listRef.current
			?.querySelector(`[data-uuid="${CSS.escape(row.uuid)}"]`)
			?.scrollIntoView({ block: "nearest" });
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
				width:
					"clamp(var(--column-min-main), calc(100vw * 5 / 14), var(--column-max-left))",
				background: "var(--dialogs-bg)",
				borderRight: "1px solid var(--shadow-fg)",
			}}
		>
			<div
				className="flex items-center px-2"
				style={{ height: "54px", borderBottom: "1px solid var(--shadow-fg)" }}
			>
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
			</div>

			<ul
				ref={listRef}
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
				{rows.map((row) => (
					<Row
						key={row.uuid}
						row={row}
						active={row.uuid === selected}
						ambiguous={duplicated.has(row.title)}
						onClick={() => onSelect(row.uuid)}
					/>
				))}
			</ul>
		</section>
	);
}

/** Deterministic avatar tint, the way Telegram colours contacts without photos. */
const TINTS = [
	"#e17076",
	"#7bc862",
	"#65aadd",
	"#a695e7",
	"#ee7aae",
	"#6ec9cb",
	"#faa774",
];
function tintFor(uuid: string): string {
	let h = 0;
	for (const ch of uuid) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
	return TINTS[h % TINTS.length] as string;
}

/** Real vaults contain untitled entries; they are not an error. */
const UNTITLED = "Untitled";

function initials(title: string): string {
	const t = title.trim();
	if (!t) return "?";
	return [...t][0]?.toUpperCase() ?? "?";
}

function relative(ms: number): string {
	const days = Math.floor((Date.now() - ms) / 86_400_000);
	if (days <= 0) return "today";
	if (days === 1) return "yesterday";
	if (days < 7) return `${days}d`;
	if (days < 365) return `${Math.floor(days / 30)}mo`;
	return `${Math.floor(days / 365)}y`;
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
	const name = active ? "var(--window-fg-active)" : "var(--dialogs-name-fg)";
	const sub = active ? "var(--window-fg-active)" : "var(--dialogs-text-fg)";

	return (
		<li data-uuid={row.uuid}>
			<button
				type="button"
				onClick={onClick}
				aria-current={active ? "true" : undefined}
				className="flex w-full cursor-pointer items-center gap-3 text-left"
				style={{
					height: "var(--dialogs-row-height)",
					padding: "0 var(--dialogs-padding-x)",
					background: active ? "var(--dialogs-bg-active)" : "transparent",
				}}
			>
				<span
					aria-hidden
					className="flex shrink-0 items-center justify-center rounded-full font-semibold"
					style={{
						width: "var(--dialogs-photo-size)",
						height: "var(--dialogs-photo-size)",
						background: active ? "rgba(255,255,255,0.22)" : tintFor(row.uuid),
						color: "#ffffff",
						fontSize: "17px",
					}}
				>
					{initials(row.title)}
				</span>

				<span className="flex min-w-0 flex-1 flex-col gap-[3px]">
					<span className="flex items-baseline gap-2">
						<span
							className="min-w-0 flex-1 truncate font-semibold"
							style={{ color: name, fontStyle: row.title ? undefined : "italic" }}
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
							<span style={{ opacity: 0.75 }}> · {row.urlHost}</span>
						)}
					</span>
				</span>
			</button>
		</li>
	);
}
