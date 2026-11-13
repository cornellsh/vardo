import { useEffect, useState } from "react";

import { humanDate } from "../lib/identity";
import { ipc } from "../ipc";
import type { HistoryVersion, Uuid } from "../ipc";
import { Presence } from "./Presence";
import { FieldLabel } from "./EntryField";

export function EntryHistory({
	uuid,
	count,
	onRestore,
}: {
	uuid: Uuid;
	count: number;
	/** Makes a version current again. Absent when the vault is read-only. */
	onRestore?: (index: number) => Promise<void>;
}) {
	const [open, setOpen] = useState(false);
	const [versions, setVersions] = useState<HistoryVersion[] | null>(null);
	const [copiedFor, setCopiedFor] = useState<number | null>(null);

	// Closed and forgotten whenever the entry changes, so old passwords for one
	// account are never sitting in memory while another is on screen.
	useEffect(() => {
		setOpen(false);
		setVersions(null);
	}, [uuid]);

	useEffect(() => {
		if (!open || versions) return;
		let cancelled = false;
		void (async () => {
			try {
				const v = await (await ipc()).history(uuid);
				if (!cancelled) setVersions(v);
			} catch {
				if (!cancelled) setVersions([]);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [open, uuid, versions]);

	useEffect(() => {
		if (copiedFor === null) return;
		const t = window.setTimeout(() => setCopiedFor(null), copiedFor * 1000);
		return () => window.clearTimeout(t);
	}, [copiedFor]);

	return (
		<div className="mt-1">
			<button
				type="button"
				onClick={() => setOpen((o) => !o)}
				aria-expanded={open}
				className="btn btn-quiet -mx-2 cursor-pointer rounded-[6px] px-2 py-1"
				style={{ color: "var(--window-active-text-fg)" }}
			>
				{count === 1 ? "1 earlier version" : `${count} earlier versions`}
			</button>

			<Presence show={open}>
				{versions === null ? (
					<p className="py-1 text-[var(--window-sub-text-fg)]">Loading…</p>
				) : (
					<div className="mt-1">
						{versions.map((v) => (
							<div
								key={v.index}
								className="group btn btn-quiet -mx-2 flex items-center gap-2 rounded-[6px] px-2 py-1.5"
							>
								<span className="min-w-0 flex-1">
									<span className="block truncate text-[var(--window-fg)]">
										{v.username || v.title || "Untitled"}
									</span>
									<FieldLabel>{humanDate(v.modified)}</FieldLabel>
								</span>
								{onRestore && (
									<button
										type="button"
										onClick={async () => {
											await onRestore(v.index);
											setVersions(null);
										}}
										className="on-hover shrink-0 cursor-pointer rounded-[6px] px-2 py-0.5"
										style={{ color: "var(--window-active-text-fg)" }}
										title="Make this version current. The current one is kept in history."
									>
										Restore
									</button>
								)}
								{v.hasPassword ? (
									<button
										type="button"
										onClick={async () => {
											const { clearAfterSeconds } = await (
												await ipc()
											).copyHistoryPassword(uuid, v.index);
											setCopiedFor(clearAfterSeconds);
										}}
										className="on-hover shrink-0 cursor-pointer rounded-[6px] px-2 py-0.5"
										style={{ color: "var(--window-active-text-fg)" }}
									>
										Copy password
									</button>
								) : (
									// Said rather than left blank. An old version with no
									// password is ordinary - a note that later gained one -
									// and a missing button reads as a broken one.
									<span
										className="shrink-0 text-[12px]"
										style={{ color: "var(--window-sub-text-fg)" }}
									>
										No password
									</span>
								)}
							</div>
						))}
						{copiedFor !== null && (
							<p className="pt-1 text-[var(--window-sub-text-fg)] text-[12px]">
								Copied. Clipboard clears in {copiedFor}s
							</p>
						)}
					</div>
				)}
			</Presence>
		</div>
	);
}

