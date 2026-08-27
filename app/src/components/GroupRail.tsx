import { useState } from "react";

import type { GroupNode, Uuid } from "../ipc";
import { useRipple } from "./Ripple";
import { DriveLogo } from "./DriveLogo";

interface Props {
	root: GroupNode | null;
	selected: Uuid | undefined;
	onSelect: (uuid: Uuid | undefined) => void;
	/** Whole-vault count, used for "All entries". */
	totalEntries: number;
	vaultName: string;
	/** Where the vault lives, in words. "Google Drive", or the folder. */
	location: string;
	/** True for a vault in Drive, which gets its mark rather than a folder. */
	remote: boolean;
	onLock: () => void;
	/** Absent on a read-only database, so no button that always fails. */
	onSync?: () => void;
	onSettings: () => void;
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
	onSettings,
	location,
	remote,
}: Props) {
	/**
	 * Which branches are open.
	 *
	 * Everything starts collapsed. A deep vault would otherwise open to a wall
	 * of nesting, and the top level is the map people actually navigate by.
	 */
	const [expanded, setExpanded] = useState<Set<string>>(new Set());

	function toggle(uuid: string) {
		setExpanded((prev) => {
			const next = new Set(prev);
			if (!next.delete(uuid)) next.add(uuid);
			return next;
		});
	}

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
				{/* The name alone said nothing about which copy you were looking
				    at. With a vault in Drive and one on disk that can share a
				    filename, "which one is this" is a question the title bar
				    should answer without being asked. */}
				<div className="flex min-w-0 items-center gap-2">
					{remote && <DriveLogo size={18} />}
					<div className="min-w-0">
						<div
							className="truncate font-semibold text-[var(--window-bold-fg)]"
							title={vaultName}
						>
							{vaultName}
						</div>
						<div
							className="truncate text-[12px] text-[var(--window-sub-text-fg)]"
							title={location}
						>
							{location}
						</div>
					</div>
				</div>
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
						onClick={onSettings}
						title="Settings"
						aria-label="Settings"
						className="flex h-7 w-7 cursor-pointer items-center justify-center rounded-[var(--button-radius)]"
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
							<circle cx="12" cy="12" r="3" />
							<path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
						</svg>
					</button>
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
				{/* The whole tree, not just its first level. Only the root's
				    direct children were drawn, so anything nested was invisible -
				    and a vault organised into Work / Clients / Acme simply had no
				    way to show you Acme. */}
				{root?.children.map((g) => (
					<GroupBranch
						key={g.uuid}
						group={g}
						depth={0}
						selected={selected}
						onSelect={onSelect}
						expanded={expanded}
						onToggle={toggle}
					/>
				))}
			</ul>
		</nav>
	);
}

/**
 * One group and everything under it.
 *
 * Recursive rather than flattened, because KDBX nesting has no depth limit and
 * a hand-unrolled two or three levels is just a lower limit that fails silently
 * on somebody's fourth.
 */
function GroupBranch({
	group,
	depth,
	selected,
	onSelect,
	expanded,
	onToggle,
}: {
	group: GroupNode;
	depth: number;
	selected: string | undefined;
	onSelect: (uuid: string | undefined) => void;
	expanded: Set<string>;
	onToggle: (uuid: string) => void;
}) {
	const hasChildren = group.children.length > 0;
	const isOpen = expanded.has(group.uuid);

	return (
		<>
			<GroupItem
				name={group.name}
				count={group.entryCount}
				active={selected === group.uuid}
				onClick={() => onSelect(group.uuid)}
				depth={depth}
				{...(hasChildren
					? { open: isOpen, onToggle: () => onToggle(group.uuid) }
					: {})}
			/>
			{isOpen &&
				group.children.map((child) => (
					<GroupBranch
						key={child.uuid}
						group={child}
						depth={depth + 1}
						selected={selected}
						onSelect={onSelect}
						expanded={expanded}
						onToggle={onToggle}
					/>
				))}
		</>
	);
}

function GroupItem({
	name,
	count,
	active,
	onClick,
	depth = 0,
	open,
	onToggle,
}: {
	name: string;
	count: number;
	active: boolean;
	onClick: () => void;
	depth?: number;
	open?: boolean;
	onToggle?: () => void;
}) {
	const ripple = useRipple();
	return (
		<li>
			<button
				type="button"
				onClick={onClick}
				aria-current={active ? "true" : undefined}
				{...ripple.handlers}
				className="relative flex w-full cursor-pointer items-center gap-2 overflow-hidden px-3 text-left"
				style={{
					height: "34px",
					// Indent by depth. 14px is enough to read as hierarchy without
					// running a deep tree off the edge of a narrow rail.
					paddingLeft: `${12 + depth * 14}px`,
					background: active ? "var(--dialogs-bg-active)" : "transparent",
					color: active ? "var(--window-fg-active)" : "var(--window-bold-fg)",
					transition: "background-color var(--duration-universal) linear",
				}}
			>
				{ripple.layer}
				{/* The twist is its own button: expanding a branch and selecting
				    it are different intentions, and a single control that did
				    both would make it impossible to look inside a group without
				    also filtering to it. */}
				{onToggle ? (
					<span
						role="button"
						tabIndex={-1}
						aria-label={open ? `Collapse ${name}` : `Expand ${name}`}
						onClick={(e) => {
							e.stopPropagation();
							onToggle();
						}}
						onKeyDown={(e) => {
							if (e.key === "Enter" || e.key === " ") {
								e.preventDefault();
								e.stopPropagation();
								onToggle();
							}
						}}
						className="relative -ml-1 flex h-5 w-4 shrink-0 items-center justify-center"
						style={{ opacity: 0.7 }}
					>
						<svg
							width="10"
							height="10"
							viewBox="0 0 10 10"
							fill="none"
							stroke="currentColor"
							strokeWidth="1.6"
							strokeLinecap="round"
							aria-hidden
							style={{
								transform: open ? "rotate(90deg)" : "none",
								transition: "transform var(--duration-universal) ease",
							}}
						>
							<path d="M3.5 1.5L7 5l-3.5 3.5" />
						</svg>
					</span>
				) : (
					<span className="relative w-3 shrink-0" aria-hidden />
				)}
				<span className="relative flex-1 truncate">{name}</span>
				{/* Empty groups show nothing rather than a zero — a "0" reads as a
            broken count, and an empty group is not an error. */}
				{count > 0 && (
					<span
						className="relative tabular-nums"
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
