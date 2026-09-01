import { useCallback, useEffect, useState } from "react";

import { EntryDetailPane } from "../components/EntryDetail";
import { EntryList } from "../components/EntryList";
import { GroupRail } from "../components/GroupRail";
import { useAutoLock } from "../hooks/useAutoLock";
import { DriveConnect } from "../components/DriveConnect";
import { MergeReview } from "./MergeReview";
import { errorText } from "../ipc/error";
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

	const [editing, setEditing] = useState(false);
	const [reload, setReload] = useState(0);
	const [reviewing, setReviewing] = useState(false);
	/** Set when the merge screen opened because a save was refused. */
	const [saveBlocked, setSaveBlocked] = useState(false);
	const [settings, setSettings] = useState(false);
	// Starts as what unlock reported and is refreshed after anything that can
	// change the vault's shape.
	const [live, setLive] = useState<VaultInfo>(info);

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

	// Auto-lock with unsaved edits would discard them. Saving first is the safe
	// direction: the write is verified and reversible via history, whereas
	// throwing away work is neither. If the save fails we do not lock, because
	// locking would then be the thing that loses the data.
	const lockAfterIdle = useCallback(async () => {
		// A failed save must not be followed by locking, or locking becomes the
		// thing that loses the data.
		try {
			if (dirty) await save();
		} catch {
			return;
		}
		onLock("idle");
	}, [dirty, save, onLock]);

	// Manual lock takes the same route. It used to lock immediately and leave a
	// "Lock and discard" button as the way to deal with unsaved work, which put
	// losing an edit one click away from keeping it. Nobody intends to throw
	// away a password they just typed, so the button is gone and locking saves.
	const lockNow = useCallback(async () => {
		try {
			if (dirty) await save();
		} catch {
			return;
		}
		onLock("manual");
	}, [dirty, save, onLock]);

	useAutoLock(AUTO_LOCK_MINUTES, lockAfterIdle);

	useEffect(() => {
		void (async () => {
			try {
				setGroups(await (await ipc()).groups());
			} catch (err) {
				setError(errorText(err));
			}
		})();
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
			if (e.key === "Escape" && query) setQuery("");
		}
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [lockNow, query, dirty, save, addEntry, info.writable]);

	// A close while dirty is the other way edits get lost. The browser needs a
	// returnValue; Tauri surfaces this as a close-request the shell can veto.
	useEffect(() => {
		if (!dirty) return;
		const warn = (e: BeforeUnloadEvent) => e.preventDefault();
		window.addEventListener("beforeunload", warn);
		return () => window.removeEventListener("beforeunload", warn);
	}, [dirty]);

	return (
		<div className="flex h-full flex-col">

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

			<div
				className="flex min-h-0 flex-1"
				style={{ background: "var(--window-bg)" }}
			>
				<GroupRail
					root={groups}
					selected={group}
					onSelect={setGroup}
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
					location={live.location}
					remote={live.source.kind === "googleDrive"}
				{...(live.storageWarning
					? {
							storageNote: live.storageWarning.explanation,
							onStorageNote: () => setSettings(true),
						}
					: {})}
				/>
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
					{...(info.writable ? { onAdd: () => void addEntry() } : {})}
				/>
				{settings ? (
					<section
						aria-label="Settings"
						className="flex min-w-0 flex-1 flex-col"
						style={{
							minWidth: "var(--column-min-third)",
							background: "var(--window-bg)",
						}}
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
						{live.storageWarning && (
							<div className="mt-4">
								<StorageNote warning={live.storageWarning} />
							</div>
						)}
						</div>
					</section>
				) : reviewing ? (
					<div
						className="flex min-w-0 flex-1 flex-col"
						style={{ minWidth: "var(--column-min-third)" }}
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
					<EntryDetailPane
						uuid={selected}
						writable={info.writable}
						onChanged={refresh}
						// Throws on failure, so the editor can keep the form open and
						// its button honest instead of closing over a failed write.
						onSaveVault={save}
						editing={editing}
						onEditing={setEditing}
					/>
				)}
			</div>
		</div>
	);
}
