import type { GroupNode, Uuid } from "../ipc";

interface Props {
	root: GroupNode | null;
	selected: Uuid | undefined;
	onSelect: (uuid: Uuid | undefined) => void;
	/** Whole-vault count, used for "All entries". */
	totalEntries: number;
	vaultName: string;
	onLock: () => void;
	/** Absent on a read-only database, so no button that always fails. */
	onSync?: () => void;
}

/**
 * Telegram's folders side bar. `sideBarBg` is darker than the list background —
 * depth runs rail → list → content in one direction, which is the opposite of
 * the usual elevation ladder and is deliberate.
 */
export function GroupRail({
	root,
	selected,
	onSelect,
	totalEntries,
	vaultName,
	onLock,
	onSync,
}: Props) {
	return (
		<nav
			aria-label="Groups"
			className="flex shrink-0 flex-col"
			style={{
				width: "var(--column-min-left)",
				background: "var(--side-bar-bg)",
				borderRight: "1px solid var(--shadow-fg)",
			}}
		>
			<header
				className="flex items-center justify-between px-3"
				style={{ height: "54px", borderBottom: "1px solid var(--shadow-fg)" }}
			>
				<span
					className="truncate font-semibold text-[var(--window-bold-fg)]"
					title={vaultName}
				>
					{vaultName}
				</span>
				<div className="flex shrink-0 items-center gap-0.5">
					{onSync && (
						<button
							type="button"
							onClick={onSync}
							title="Review and sync"
							aria-label="Review and sync"
							className="flex h-7 w-7 cursor-pointer items-center justify-center rounded-[6px]"
							style={{ color: "var(--window-sub-text-fg)" }}
						>
							<svg
								width="16"
								height="16"
								viewBox="0 0 24 24"
								fill="none"
								stroke="currentColor"
								strokeWidth="2"
								strokeLinecap="round"
								strokeLinejoin="round"
								aria-hidden
							>
								<path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-7.5-4M3 12a9 9 0 0 1 9-9 9 9 0 0 1 7.5 4" />
								<path d="M20 4v5h-5M4 20v-5h5" />
							</svg>
						</button>
					)}
					<button
						type="button"
						onClick={onLock}
						title="Lock (Ctrl+L)"
						aria-label="Lock vault"
						className="flex h-7 w-7 cursor-pointer items-center justify-center rounded-[6px]"
						style={{ color: "var(--window-sub-text-fg)" }}
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
							<rect x="4" y="10" width="16" height="10" rx="2" />
							<path d="M8 10V7a4 4 0 0 1 8 0v3" />
						</svg>
					</button>
				</div>
			</header>

			<ul className="flex-1 overflow-y-auto py-1">
				<GroupItem
					name="All entries"
					count={totalEntries}
					active={selected === undefined}
					onClick={() => onSelect(undefined)}
				/>
				{root?.children.map((g) => (
					<GroupItem
						key={g.uuid}
						name={g.name}
						count={g.entryCount}
						active={selected === g.uuid}
						onClick={() => onSelect(g.uuid)}
					/>
				))}
			</ul>
		</nav>
	);
}

function GroupItem({
	name,
	count,
	active,
	onClick,
}: {
	name: string;
	count: number;
	active: boolean;
	onClick: () => void;
}) {
	return (
		<li>
			<button
				type="button"
				onClick={onClick}
				aria-current={active ? "true" : undefined}
				className="flex w-full cursor-pointer items-center gap-2 px-3 text-left"
				style={{
					height: "34px",
					background: active ? "var(--dialogs-bg-active)" : "transparent",
					color: active ? "var(--window-fg-active)" : "var(--window-bold-fg)",
				}}
			>
				<span className="flex-1 truncate">{name}</span>
				{/* Empty groups show nothing rather than a zero — a "0" reads as a
            broken count, and an empty group is not an error. */}
				{count > 0 && (
					<span
						className="tabular-nums"
						style={{
							color: active
								? "var(--window-fg-active)"
								: "var(--window-sub-text-fg)",
						}}
					>
						{count}
					</span>
				)}
			</button>
		</li>
	);
}
