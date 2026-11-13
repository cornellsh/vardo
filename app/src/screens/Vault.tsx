import { useCallback, useEffect, useMemo, useState } from "react";

import { EntryDetailPane } from "../components/EntryDetail";
import { Notice } from "../components/Notice";
import { CommandPalette } from "../components/CommandPalette";
import type { Command } from "../components/CommandPalette";
import { EntryList } from "../components/EntryList";
import { GroupRail } from "../components/GroupRail";
import type { GroupAction } from "../components/GroupRail";
import { useAutoLock } from "../hooks/useAutoLock";
import { DriveConnect } from "../components/DriveConnect";
import { BrowserConnect } from "../components/BrowserConnect";
import { IconFetch } from "../components/IconFetch";
import { MergeReview } from "./MergeReview";
import { errorText } from "../ipc/error";
import { saveThenLock } from "../lib/saveThenLock";
import { findGroup, groupByName } from "../lib/groupByName";
import { useLayoutMode } from "../lib/useLayoutMode";
import { Health } from "./Health";
import { usePresence } from "../lib/usePresence";
import { ipc } from "../ipc";
import type { EntrySummary, GroupNode, Uuid, VaultInfo } from "../ipc";
import { StorageNote } from "../components/StorageNote";

interface Props {
	info: VaultInfo;
	onLock: (reason: "idle" | "manual") => void;
}

/** Minutes of inactivity before the vault locks itself. */
const AUTO_LOCK_MINUTES = 5;

export function Vault({ info, onLock }: Props) {
	const [groups, setGroups] = useState<GroupNode | null>(null);
	const [group, setGroup] = useState<Uuid | undefined>(undefined);
	const [query, setQuery] = useState("");
	/**
	 * Remembered for the session. Someone who wants recent-first wants it for
	 * every group they look at, and resetting it on each click would be a
	 * setting that never stays set.
	 */
	const [sort, setSort] = useState<"name" | "recent">("name");
	const [rows, setRows] = useState<EntrySummary[]>([]);
	const [selected, setSelected] = useState<Uuid | null>(null);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [dirty, setDirty] = useState(false);
	const mode = useLayoutMode();
	/** Whether the group drawer is showing, when it is a drawer at all. */
	const [railOpen, setRailOpen] = useState(false);

	// A drawer left open across a resize would sit on top of a rail that is now
	// a permanent column, which is two copies of the same thing.
	useEffect(() => {
		if (mode === "three") setRailOpen(false);
	}, [mode]);

	// Held mounted while it slides back out. `slideWrapDuration` must match the
	// exit animation below, or the element is torn out mid-movement.
	const drawer = usePresence(railOpen, 150);

	const [editing, setEditing] = useState(false);
	const [reload, setReload] = useState(0);
	const [reviewing, setReviewing] = useState(false);
	/** Set when the merge screen opened because a save was refused. */
	const [saveBlocked, setSaveBlocked] = useState(false);
	const [settings, setSettings] = useState(false);
	const [health, setHealth] = useState(false);
	/**
	 * Problems found, so the rail can carry the number without being opened.
	 *
	 * A count in the rail is the only thing that makes this feature exist for
	 * anyone: a screen nobody visits reports nothing, however good it is.
	 */
	const [healthCount, setHealthCount] = useState<number | null>(null);
	// Starts as what unlock reported and is refreshed after anything that can
	// change the vault's shape.
	const [live, setLive] = useState<VaultInfo>(info);
	const [elsewhereSeen, setElsewhereSeen] = useState(false);
	const [palette, setPalette] = useState(false);

	/**
	 * Writes the vault, and throws if it could not.
	 *
	 * It used to swallow the reason and return false, which was fine while a bar
	 * at the top of the window held the message. Now that failures are shown
	 * beside whatever caused them, the caller needs the actual error rather than
	 * a boolean it can only turn back into "something went wrong".
	 */
	const save = useCallback(async () => {
		try {
			await (await ipc()).saveVault();
			setDirty(false);
		} catch (err) {
			// A vault changed elsewhere is not a failure and must not read like
			// one. Nothing is wrong, there is simply a newer copy, and the answer
			// is the merge screen rather than a message about a broken save.
			if ((err as { kind?: string })?.kind === "remote-changed") {
				setSaveBlocked(true);
				setReviewing(true);
			}
			throw err;
		}
	}, []);

	const lockAfterIdle = useCallback(
		async () => void (await saveThenLock(dirty, save, () => onLock("idle"))),
		[dirty, save, onLock],
	);

	// Manual lock takes the same route. It used to lock immediately and leave a
	// "Lock and discard" button as the way to deal with unsaved work, which put
	// losing an edit one click away from keeping it. Nobody intends to throw
	// away a password they just typed, so the button is gone and locking saves.
	/**
	 * Create, rename or delete a group.
	 *
	 * `prompt` and `confirm` rather than bespoke dialogs. They are the shell's
	 * own, they are keyboard-correct and translated for free, and building three
	 * modal boxes to replace them would be work spent on the part of this that
	 * nobody will remember. Worth revisiting if they ever need to say more than
	 * a sentence.
	 *
	 * Every branch saves, like every other change.
	 */

	const lockNow = useCallback(
		async () => void (await saveThenLock(dirty, save, () => onLock("manual"))),
		[dirty, save, onLock],
	);

	useAutoLock(AUTO_LOCK_MINUTES, lockAfterIdle);

	// The window says which vault is open. It said "Vardo" always, which is no
	// help at all to someone with a work vault and a personal one side by side
	// in a task switcher - the one place a title is actually read.
	//
	// The name only. Not the path, which can be long and is nobody else's
	// business when a screen is being shared.
	useEffect(() => {
		document.title = `${live.name} \u2014 Vardo`;
		return () => {
			document.title = "Vardo";
		};
	}, [live.name]);

	useEffect(() => {
		void (async () => {
			try {
				setGroups(await (await ipc()).groups());
			} catch (err) {
				setError(errorText(err));
			}
		})();
	}, [reload]);

	// Recounted whenever the vault changes, because the number is only worth
	// showing if it responds to fixing something. A count that stayed put after
	// you changed a reused password would teach you to ignore it.
	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const report = await (await ipc()).health();
				if (!cancelled) setHealthCount(report.findings.length);
			} catch {
				// Never blocks the vault. A failed count is a missing badge, not
				// a reason to interrupt someone looking for a password.
				if (!cancelled) setHealthCount(null);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [reload]);

	const refresh = useCallback(() => {
		setReload((n) => n + 1);
		void (async () => {
			const api = await ipc();
			setDirty(await api.isDirty());
			// A merge rewrites the vault in place, so the count handed out at
			// unlock is no longer true.
			try {
				setLive(await api.vaultInfo());
			} catch {
				/* locked; the unlock screen takes over anyway */
			}
		})();
	}, []);

	// A login saved from the browser extension is written like any other edit.
	// A refused save opens the merge screen through `save` itself.
	useEffect(() => {
		let dispose: (() => void) | undefined;
		void (async () => {
			dispose = await (await ipc()).onVaultChanged?.(() => {
				refresh();
				void save().catch(() => {});
			});
		})();
		return () => dispose?.();
	}, [refresh, save]);

	const groupAction = useCallback(
		(action: GroupAction, group: GroupNode) => {
			void (async () => {
				const api = await ipc();
				try {
					if (action === "create") {
						const name = window.prompt(`New group inside ${group.name}`);
						if (!name?.trim()) return;
						await api.createGroup(group.uuid, name.trim());
					} else if (action === "move") {
						// The keyboard's way to do what dragging does.
						const name = window.prompt(
							`Move ${group.name} into which group? Leave empty for the top level.`,
						);
						if (name === null || !groups) return;
						const target = groupByName(groups, name);
						if (typeof target === "string") {
							setError(target);
							return;
						}
						await api.moveGroup(group.uuid, target.uuid);
					} else if (action === "rename") {
						const name = window.prompt("Rename group", group.name);
						if (!name?.trim() || name === group.name) return;
						await api.renameGroup(group.uuid, name.trim());
					} else {
						// The count is the whole content of the warning. "Delete this
						// group?" and "delete this group and the 38 passwords inside
						// it?" are different questions, and only one of them can be
						// answered honestly without knowing the number.
						const count = await api.groupEntryCount(group.uuid);
						const warning =
							count === 0
								? `Delete the group ${group.name}?`
								: `Delete ${group.name} and the ${count} ${
										count === 1 ? "entry" : "entries"
									} inside it?`;
						if (!window.confirm(warning)) return;
						await api.deleteGroup(group.uuid);
						// The group being looked at has just gone, so the list would
						// otherwise filter to a group that no longer exists.
						setGroup(undefined);
						setSelected(null);
					}
					await save();
					refresh();
				} catch (err) {
					setError(errorText(err));
				}
			})();
		},
		[save, refresh, groups],
	);

	const moveGroup = useCallback(
		(uuid: Uuid, parent: Uuid) => {
			void (async () => {
				try {
					await (await ipc()).moveGroup(uuid, parent);
					await save();
					refresh();
				} catch (err) {
					setError(errorText(err));
				}
			})();
		},
		[save, refresh],
	);

	const addEntry = useCallback(async () => {
		const api = await ipc();
		// A new entry belongs somewhere concrete; "All entries" is a view, not a
		// group, so fall back to the root.
		const target = group ?? groups?.uuid;
		if (!target) return;
		const uuid = await api.createEntry(target);
		refresh();
		setSelected(uuid);
		setEditing(true);
	}, [group, groups, refresh]);

	useEffect(() => {
		let cancelled = false;
		setLoading(true);
		setError(null);
		void (async () => {
			try {
				const api = await ipc();
				const next = await api.listEntriesSorted(
					group ?? null,
					query || null,
					sort,
				);
				if (cancelled) return;
				setRows(next);
				// Keep the selection only if it survived the filter, so the detail
				// pane never shows something the list no longer contains.
				setSelected((cur) =>
					cur && next.some((r) => r.uuid === cur) ? cur : null,
				);
			} catch (err) {
				// Without this the list sits on "Loading…" forever, which is
				// indistinguishable from a slow vault and hides the real fault.
				if (!cancelled) setError(errorText(err));
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [group, query, sort, reload]);

	useEffect(() => {
		function onKey(e: KeyboardEvent) {
			const mod = e.ctrlKey || e.metaKey;
			if (e.key === "k" && mod) {
				e.preventDefault();
				setPalette((open) => !open);
				return;
			}
			if (e.key === "s" && mod) {
				e.preventDefault();
				// Kept, because muscle memory is real and the shortcut existing
				// costs nothing. It is a safety net rather than the way anything
				// gets saved: every change already writes where it is made, so
				// `dirty` is normally false and this does nothing. A rejection is
				// surfaced by whatever the user was doing, not from here.
				if (dirty) void save().catch(() => {});
				return;
			}
			if (e.key === "n" && mod && info.writable) {
				e.preventDefault();
				void addEntry();
				return;
			}
			if (e.key === "l" && mod) {
				e.preventDefault();
				// Saves first, like every other route to locking. This used to do
				// nothing at all while there were unsaved edits, so the shortcut
				// appeared broken at exactly the moment it mattered most.
				void lockNow();
				return;
			}
			if (e.key === "Escape") {
				// Innermost thing first. A drawer laid over the content is what
				// Escape most obviously means while it is open, and clearing the
				// search behind it instead would look like the key did nothing.
				if (railOpen) setRailOpen(false);
				else if (query) setQuery("");
			}
		}
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [lockNow, query, dirty, save, addEntry, info.writable, railOpen]);

	// A close while dirty is the other way edits get lost. The browser needs a
	// returnValue; Tauri surfaces this as a close-request the shell can veto.
	useEffect(() => {
		if (!dirty) return;
		const warn = (e: BeforeUnloadEvent) => e.preventDefault();
		window.addEventListener("beforeunload", warn);
		return () => window.removeEventListener("beforeunload", warn);
	}, [dirty]);

	// Each command reuses the callback behind a button or shortcut, so the
	// palette cannot do something the window itself would not.
	const commands = useMemo<Command[]>(() => {
		const list: Command[] = [];
		if (info.writable) list.push({ id: "new", label: "New entry", shortcut: "Ctrl+N", run: () => void addEntry() });
		if (info.writable && live.source.kind === "googleDrive") {
			list.push({
				id: "sync",
				label: "Sync with Google Drive",
				run: () => {
					setSaveBlocked(false);
					setReviewing(true);
				},
			});
		}
		list.push({
			id: "health",
			label: "Password health",
			run: () => {
				setSettings(false);
				setReviewing(false);
				setHealth(true);
				setRailOpen(false);
			},
		});
		list.push({ id: "settings", label: "Settings", run: () => setSettings(true) });
		if (mode !== "three") list.push({ id: "groups", label: "Show groups", run: () => setRailOpen(true) });
		// The selected group's actions, so they are not right-click only: macOS
		// has no key that opens a context menu.
		const node = info.writable && groups && group ? findGroup(groups, group) : undefined;
		if (node) {
			const actions: [GroupAction, string][] = [
				["create", `New group inside ${node.name}`],
				["rename", `Rename group ${node.name}…`],
				["move", `Move group ${node.name}…`],
				["delete", `Delete group ${node.name}…`],
			];
			for (const [action, label] of actions) {
				list.push({ id: `group-${action}`, label, run: () => groupAction(action, node) });
			}
		}
		list.push({ id: "lock", label: "Lock now", shortcut: "Ctrl+L", run: () => void lockNow() });
		return list;
	}, [info.writable, live.source.kind, mode, addEntry, lockNow, groups, group, groupAction]);

	return (
		<div className="flex h-full flex-col">
			{palette && <CommandPalette commands={commands} onClose={() => setPalette(false)} />}
			{/* There is no unsaved-changes bar.

			    There was one, and it made no sense once you looked at it: a strip
			    across the top offering Save beside "Lock and discard", while the
			    editor you had just used had its own confirm and cancel. Two places
			    to commit one edit, two vocabularies for it, and losing the work one
			    click from keeping it at the same size.

			    It existed because saving was a second step. It is not any more:
			    every change writes where it is made, so there is no in-between
			    state left for a bar to describe. Failures appear next to whatever
			    failed, and locking saves rather than asking. */}

			{live.openElsewhere && !elsewhereSeen && (
				<div className="shrink-0 px-3 pt-3">
					<Notice
						tone="warning"
						title="This vault is open in another Vardo window"
						detail="You can edit here. If both windows save, the later save stops and asks you to merge. Locking the other window avoids that."
						action={{ label: "Got it", onClick: () => setElsewhereSeen(true) }}
					/>
				</div>
			)}

			<div
				className="relative flex min-h-0 flex-1"
				style={{ background: "var(--window-bg)" }}
			>
				{/* Below three panes the rail becomes a drawer over the content
				    rather than a column beside it. Groups need their names, so
				    shrinking it to icons was never an option; the choice is between
				    showing it and not, and most of the time you are not changing
				    group. */}
				{drawer.mounted && mode !== "three" && (
					<button
						type="button"
						aria-label="Close groups"
						onClick={() => setRailOpen(false)}
						className="absolute inset-0 z-10 cursor-default"
						style={{
							background: "rgba(0,0,0,0.45)",
							animation: drawer.visible
								? "fade-in var(--duration-fade-wrap) linear"
								: "fade-out var(--duration-slide-wrap) linear forwards",
						}}
					/>
				)}

				{(mode === "three" || drawer.mounted) && (
					<div
						className={
							mode === "three"
								? "contents"
								: "absolute inset-y-0 left-0 z-20 flex"
						}
						style={
							mode === "three"
								? undefined
								: {
										boxShadow: "0 0 24px rgba(0,0,0,0.5)",
										// Out faster than in. Arriving is the part worth
										// watching; leaving should get out of the way.
										animation: drawer.visible
											? "slide-in-left var(--duration-slide) var(--ease-out-circ)"
											: "slide-out-left var(--duration-slide-wrap) var(--ease-out-circ) forwards",
									}
						}
					>
						<GroupRail
							root={groups}
							selected={group}
							// Choosing a group closes the drawer, because the drawer is
							// covering the list it just filtered. Leaving it open would hide
							// the only evidence that the choice did anything.
							onSelect={(uuid) => {
								setGroup(uuid);
								setRailOpen(false);
							}}
							// The vault's own count, not the root group's direct children:
							// entries almost always live in subgroups, so root's own count is
							// usually 0 and "All entries" would read as empty.
							totalEntries={live.entryCount}
							vaultName={live.name}
							onLock={() => void lockNow()}
							// Sync belongs to a vault something else can change, which means
							// a vault in Drive. Offering it on a file on this machine's disk
							// merged two models: there is no other copy to reconcile, so the
							// button could only compare the vault against itself.
							//
							// A local file can still change underneath us - another window,
							// another program - and that is handled where it happens, by the
							// save refusing and offering the merge then. Reactive, because
							// nobody sits down intending to sync a local file.
							{...(info.writable && live.source.kind === "googleDrive"
								? {
										onSync: () => {
											setSaveBlocked(false);
											setReviewing(true);
										},
									}
								: {})}
							onSettings={() => setSettings((s) => !s)}
							{...(info.writable ? { onGroupAction: groupAction, onMoveGroup: moveGroup } : {})}
							onHealth={() => {
								setSettings(false);
								setReviewing(false);
								setHealth(true);
								setRailOpen(false);
							}}
							{...(healthCount !== null ? { healthCount } : {})}
							location={live.location}
							remote={live.source.kind === "googleDrive"}
							{...(live.storageWarning
								? {
										storageNote: live.storageWarning.explanation,
										onStorageNote: () => setSettings(true),
									}
								: {})}
						/>
					</div>
				)}

				{/* At one pane the list and the detail take turns. Selecting an
				    entry replaces the list, and the detail pane's back control
				    returns to it - the same trade every mail client makes, because
				    380 and 292 side by side do not fit in 600 and something has to
				    give. */}
				{(mode !== "one" || (!selected && !settings && !reviewing)) && (
					<EntryList
						rows={rows}
						loading={loading}
						error={error}
						query={query}
						onQuery={setQuery}
						sort={sort}
						onSort={setSort}
						selected={selected}
						onSelect={setSelected}
						fill={mode === "one"}
						{...(mode !== "three"
							? { onOpenGroups: () => setRailOpen(true) }
							: {})}
						{...(info.writable ? { onAdd: () => void addEntry() } : {})}
					/>
				)}
				{health ? (
					<Health
						key="health"
						onClose={() => setHealth(false)}
						onOpenEntry={(uuid) => {
							// Clearing the group first, or an entry from a group you are
							// not looking at is selected and then filtered straight back
							// out, which looks like the click did nothing.
							setGroup(undefined);
							setQuery("");
							setSelected(uuid);
							setHealth(false);
						}}
					/>
				) : settings ? (
					<section
						aria-label="Settings"
						className="flex min-w-0 flex-1 flex-col"
						style={{
							minWidth: "var(--column-min-third)",
							background: "var(--window-bg)",
							// Keyed below, so switching between the three things that can
							// occupy this column replays the entrance instead of swapping
							// contents inside a box that never moves.
							animation:
								"fade-in var(--duration-fade-wrap) var(--ease-out-circ)",
						}}
						key="settings"
					>
						<header
							className="flex shrink-0 items-center px-4"
							style={{
								height: "54px",
								borderBottom: "1px solid var(--shadow-fg)",
							}}
						>
							<h2 className="flex-1 font-semibold text-[15px] text-[var(--window-bold-fg)]">
								Settings
							</h2>
							<button
								type="button"
								onClick={() => setSettings(false)}
								className="btn btn-quiet cursor-pointer rounded-[var(--button-radius)] px-2 py-1"
								style={{ color: "var(--window-sub-text-fg)" }}
							>
								Close
							</button>
						</header>
						<div
							className="min-h-0 flex-1 overflow-y-auto p-4"
							style={{
								animation:
									"slide-up var(--duration-slide-wrap) var(--ease-out-circ)",
							}}
						>
							<DriveConnect />
							<div className="mt-4">
								<BrowserConnect />
							</div>
							{info.writable && (
								<div className="mt-4">
									<IconFetch onProgress={refresh} onFinished={save} />
								</div>
							)}
							{live.storageWarning && (
								<div className="mt-4">
									<StorageNote warning={live.storageWarning} />
								</div>
							)}
						</div>
					</section>
				) : reviewing ? (
					<div
						key="review"
						className="flex min-w-0 flex-1 flex-col"
						style={{
							minWidth: "var(--column-min-third)",
							animation:
								"fade-in var(--duration-fade-wrap) var(--ease-out-circ)",
						}}
					>
						<MergeReview
							remote={live.source.kind === "googleDrive"}
							{...(saveBlocked ? { reason: "save-blocked" as const } : {})}
							onClose={() => {
								setSaveBlocked(false);
								setReviewing(false);
							}}
							onMerged={() => {
								// The merge rewrote the vault in place, so everything on
								// screen is stale.
								setSaveBlocked(false);
								setSelected(null);
								refresh();
							}}
						/>
					</div>
				) : (
					// At one pane this only appears once an entry is chosen. Rendering
					// it alongside the list squeezed the list to 88 pixels at 380 wide
					// - no overflow, and no usable list either, which is a worse
					// failure than the overflow it replaced because nothing looks
					// broken enough to report.
					(mode !== "one" || !!selected) && (
						<EntryDetailPane
							uuid={selected}
							writable={info.writable}
							onChanged={refresh}
							// Throws on failure, so the editor can keep the form open and
							// its button honest instead of closing over a failed write.
							onSaveVault={save}
							{...(mode === "one" ? { onBack: () => setSelected(null) } : {})}
							editing={editing}
							onEditing={setEditing}
						/>
					)
				)}
			</div>
		</div>
	);
}
