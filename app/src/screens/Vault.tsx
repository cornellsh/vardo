import { useCallback, useEffect, useState } from "react";

import { EntryDetailPane } from "../components/EntryDetail";
import { EntryList } from "../components/EntryList";
import { GroupRail } from "../components/GroupRail";
import { useAutoLock } from "../hooks/useAutoLock";
import { MergeReview } from "./MergeReview";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { EntrySummary, GroupNode, Uuid, VaultInfo } from "../ipc";

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
	const [rows, setRows] = useState<EntrySummary[]>([]);
	const [selected, setSelected] = useState<Uuid | null>(null);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [dirty, setDirty] = useState(false);
	const [saving, setSaving] = useState(false);
	const [saveError, setSaveError] = useState<string | null>(null);
	const [editing, setEditing] = useState(false);
	const [reload, setReload] = useState(0);
	const [reviewing, setReviewing] = useState(false);
	// Starts as what unlock reported and is refreshed after anything that can
	// change the vault's shape.
	const [live, setLive] = useState<VaultInfo>(info);

	const save = useCallback(async () => {
		setSaving(true);
		setSaveError(null);
		try {
			await (await ipc()).saveVault();
			setDirty(false);
			return true;
		} catch (err) {
			// The core verifies its own output before replacing anything, so a
			// failed save means the file on disk is untouched. Say so, because
			// "save failed" otherwise reads as "your vault may be broken".
			setSaveError(`${errorText(err)} — the file on disk is unchanged.`);
			return false;
		} finally {
			setSaving(false);
		}
	}, []);

	// Auto-lock with unsaved edits would discard them. Saving first is the safe
	// direction: the write is verified and reversible via history, whereas
	// throwing away work is neither. If the save fails we do not lock, because
	// locking would then be the thing that loses the data.
	const lockAfterIdle = useCallback(async () => {
		if (dirty && !(await save())) return;
		onLock("idle");
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
				const next = await api.listEntries({
					...(group ? { group } : {}),
					...(query ? { query } : {}),
				});
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
	}, [group, query, reload]);

	useEffect(() => {
		function onKey(e: KeyboardEvent) {
			const mod = e.ctrlKey || e.metaKey;
			if (e.key === "s" && mod) {
				e.preventDefault();
				if (dirty) void save();
				return;
			}
			if (e.key === "n" && mod && info.writable) {
				e.preventDefault();
				void addEntry();
				return;
			}
			if (e.key === "l" && mod) {
				e.preventDefault();
				// Manual lock is an explicit request, so it does not silently save;
				// the confirmation bar makes the choice instead.
				if (!dirty) onLock("manual");
				return;
			}
			if (e.key === "Escape" && query) setQuery("");
		}
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [onLock, query, dirty, save, addEntry, info.writable]);

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
			{(dirty || saveError) && (
				<div
					className="flex shrink-0 items-center gap-3 px-4"
					style={{
						height: "38px",
						background: saveError
							? "var(--window-bg-over)"
							: "var(--active-button-bg)",
						color: saveError
							? "var(--attention-button-fg)"
							: "var(--window-fg-active)",
					}}
				>
					<span className="min-w-0 flex-1 truncate">
						{saveError ?? "Unsaved changes"}
					</span>
					{dirty && (
						<>
							<button
								type="button"
								disabled={saving}
								onClick={() => void save()}
								className="shrink-0 cursor-pointer rounded-[6px] px-2 py-1 font-semibold disabled:opacity-60"
								style={{
									background: "rgba(255,255,255,0.18)",
									color: "inherit",
								}}
							>
								{saving ? "Saving…" : "Save  (Ctrl+S)"}
							</button>
							<button
								type="button"
								onClick={() => onLock("manual")}
								className="shrink-0 cursor-pointer rounded-[6px] px-2 py-1"
								style={{ color: "inherit", opacity: 0.85 }}
							>
								Lock and discard
							</button>
						</>
					)}
				</div>
			)}

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
					onLock={() => onLock("manual")}
					{...(info.writable ? { onSync: () => setReviewing(true) } : {})}
				/>
				<EntryList
					rows={rows}
					loading={loading}
					error={error}
					query={query}
					onQuery={setQuery}
					selected={selected}
					onSelect={setSelected}
					{...(info.writable ? { onAdd: () => void addEntry() } : {})}
				/>
			{reviewing ? (
				<div
					className="flex min-w-0 flex-1 flex-col"
					style={{ minWidth: "var(--column-min-third)" }}
				>
					<MergeReview
						onClose={() => setReviewing(false)}
						onMerged={() => {
							// The merge rewrote the vault in place, so everything on
							// screen is stale.
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
					editing={editing}
					onEditing={setEditing}
				/>
			)}
			</div>
		</div>
	);
}
