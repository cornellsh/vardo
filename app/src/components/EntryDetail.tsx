import { useEffect, useRef, useState } from "react";

import { humanDate, identityOf, initialOf, tintFor } from "../lib/identity";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { EntryDetail, Uuid } from "../ipc";
import { EntryEditor } from "./EntryEditor";
import { Notice, splitTechnical } from "./Notice";
import { Presence } from "./Presence";
import { TotpRow } from "./EntryTotp";
import { AttachmentRow } from "./EntryAttachment";
import { EntryHistory } from "./EntryHistory";
import { GroupHeading, EmptyState, FieldRow } from "./EntryField";

interface Props {
	uuid: Uuid | null;
	writable: boolean;
	/** Called after any change, so the list and dirty state refresh. */
	onChanged: () => void;
	/** Writes the vault to its source. Rejects if the write failed. */
	onSaveVault: () => Promise<void>;
	/** Returns to the list. Only set when this pane replaced it. */
	onBack?: (() => void) | undefined;
	/** Opened straight into edit mode, for a freshly created entry. */
	editing: boolean;
	onEditing: (editing: boolean) => void;
}

export function EntryDetailPane({
	uuid,
	writable,
	onChanged,
	onSaveVault,
	onBack,
	editing,
	onEditing,
}: Props) {
	const [entry, setEntry] = useState<EntryDetail | null>(null);
	const [reload, setReload] = useState(0);
	const [confirmingDelete, setConfirmingDelete] = useState(false);
	/** A failed delete, shown here rather than in a bar somewhere else. */
	const [actionError, setActionError] = useState<string | null>(null);
	/**
	 * The last message, kept after `actionError` clears.
	 *
	 * Something has to hold the text while the notice animates away, or the
	 * exit plays on an empty box.
	 */
	const [lastActionError, setLastActionError] = useState("");
	/**
	 * Entry and address pairs already tried, so a site with no icon is asked once
	 * per session, and saving a new address asks again.
	 */
	const tried = useRef(new Set<string>());

	useEffect(() => {
		if (!uuid) {
			setEntry(null);
			return;
		}
		let cancelled = false;
		void (async () => {
			const api = await ipc();
			const next = await api.getEntry(uuid);
			if (cancelled) return;
			setEntry(next);

			// The icon for the entry being looked at, and only that one.
			//
			// Doing this on unlock instead would fire a request at every domain in
			// the vault at once: a burst nobody asked for, and a decent map of
			// someone's life leaving the machine in one go. One request, for the
			// entry they just opened, is traffic that site was going to see from
			// this person anyway.
			//
			// The result is written into the vault, so it happens once ever rather
			// than once per device or once per visit.
			const url = next?.fields.find((f) => f.key === "URL")?.value ?? "";
			const attempt = `${uuid}\n${url}`;
			if (next && !next.iconPng && url && writable && !tried.current.has(attempt)) {
				tried.current.add(attempt);
				try {
					const png = await api.fetchIcon(uuid);
					if (!cancelled && png) {
						setEntry((e) =>
							e && e.uuid === uuid ? { ...e, iconPng: png } : e,
						);
						onChanged();
					}
				} catch {
					// A site being unreachable is not worth a message. The entry is
					// perfectly usable with a letter for an avatar.
				}
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [uuid, reload]);

	useEffect(() => setConfirmingDelete(false), [uuid]);

	const [attaching, setAttaching] = useState(false);

	async function attach() {
		if (!uuid) return;
		setActionError(null);
		try {
			const api = await ipc();
			const path = await api.pickAnyFile();
			if (!path) return;
			setAttaching(true);
			await api.addAttachment(uuid, path);
			// Stored immediately, like every other change. An attachment that
			// lived only in memory would be lost by a lock, and the one thing
			// people attach is the thing they cannot regenerate.
			await onSaveVault();
			setReload((n) => n + 1);
			onChanged();
		} catch (err) {
			const text = errorText(err);
			setLastActionError(text);
			setActionError(text);
		} finally {
			setAttaching(false);
		}
	}

	/** An ordinary edit: the version replaced goes into history. */
	async function restore(index: number) {
		if (!uuid) return;
		setActionError(null);
		try {
			await (await ipc()).restoreVersion(uuid, index);
			await onSaveVault();
			setReload((n) => n + 1);
			onChanged();
		} catch (err) {
			const text = errorText(err);
			setLastActionError(text);
			setActionError(text);
		}
	}

	async function remove() {
		if (!uuid) return;
		setActionError(null);
		try {
			await (await ipc()).deleteEntry(uuid);
			// Stored straight away, like every other change. A deletion that
			// lived only in memory left the vault in a state no screen described:
			// the entry gone from the list, still present in the file.
			await onSaveVault();
			setConfirmingDelete(false);
			onChanged();
		} catch (err) {
			const text = errorText(err);
			setLastActionError(text);
			setActionError(text);
		}
	}

	return (
		<section
			aria-label="Entry"
			className="flex min-w-0 flex-1 flex-col"
			style={{
				background: "var(--window-bg)",
				minWidth: "var(--column-min-third)",
			}}
		>
			{!entry ? (
				<EmptyState />
			) : (
				// Keyed on the entry, so moving down the list replays the entrance
				// rather than swapping text inside a pane that never moves. Without
				// it, arrowing through entries changed everything at once with no
				// sense that anything had been navigated.
				//
				// Entry only, no exit: the outgoing entry is replaced in the same
				// frame by the incoming one, so there is nothing to animate away and
				// holding the old one would show two entries at once.
				<div
					key={entry.uuid}
					className="flex min-h-0 flex-1 flex-col"
					style={{
						animation: "fade-in var(--duration-universal) var(--ease-out-circ)",
					}}
				>
					<header
						className="flex items-center gap-3 px-4"
						style={{
							height: "54px",
							borderBottom: "1px solid var(--shadow-fg)",
						}}
					>
						{/* The same avatar as the list row it came from. Telegram
						    carries the peer's identity into the info panel rather
						    than dropping to a bare title, which is what makes the
						    two feel like one place instead of two screens. */}
						{/* The way back, when this pane has replaced the list rather
						    than sitting beside it. Without it a narrow window is a
						    one-way trip into an entry. */}
						{onBack && (
							<button
								type="button"
								onClick={onBack}
								aria-label="Back to the list"
								title="Back"
								className="btn btn-quiet -ml-1 flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full"
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
									<path d="M15 5l-7 7 7 7" />
								</svg>
							</button>
						)}
						<span
							aria-hidden
							className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold"
							style={{
								background: entry.iconPng ? "#ffffff" : tintFor(
									identityOf({
										title: entry.title,
										url:
											entry.fields.find((f) => f.key === "URL")?.value ?? null,
									}),
								),
								color: "#ffffff",
								fontSize: "15px",
							}}
						>
							{entry.iconPng ? (
								<img
									src={entry.iconPng}
									alt=""
									style={{ width: "100%", height: "100%", objectFit: "cover" }}
								/>
							) : (
								initialOf(entry.title || "?")
							)}
						</span>

						<div className="min-w-0 flex-1">
							<h2
								className="truncate font-semibold text-[var(--window-bold-fg)] text-[15px]"
								style={{ fontStyle: entry.title ? undefined : "italic" }}
							>
								{entry.title || "Untitled"}
							</h2>
							<p className="truncate text-[var(--window-sub-text-fg)] text-[12px]">
								{entry.groupPath.join(" / ")}
							</p>
						</div>

						{writable && !editing && (
							<div className="flex shrink-0 items-center gap-1">
								<button
									type="button"
									onClick={() => onEditing(true)}
									className="btn btn-quiet cursor-pointer rounded-[6px] px-2 py-1"
									style={{ color: "var(--window-active-text-fg)" }}
								>
									Edit
								</button>
								{confirmingDelete ? (
									<>
										{/* Inline confirmation rather than a modal: it keeps the
										    entry visible while deciding, which a dialog covering
										    it does not. */}
										<button
											type="button"
											onClick={() => void remove()}
											className="btn btn-quiet cursor-pointer rounded-[6px] px-2 py-1 font-semibold"
											style={{ color: "var(--attention-button-fg)" }}
										>
											Confirm
										</button>
										<button
											type="button"
											onClick={() => setConfirmingDelete(false)}
											className="btn btn-quiet cursor-pointer rounded-[6px] px-2 py-1"
											style={{ color: "var(--window-sub-text-fg)" }}
										>
											Keep
										</button>
									</>
								) : (
									<button
										type="button"
										onClick={() => setConfirmingDelete(true)}
										className="btn btn-quiet cursor-pointer rounded-[6px] px-2 py-1"
										style={{ color: "var(--attention-button-fg)" }}
									>
										Delete
									</button>
								)}
							</div>
						)}
					</header>

					{/* Failures appear next to the thing that failed, rather than in a
					    bar at the top of the window. A delete that could not be stored
					    is about this entry, and the entry is on screen. */}
					<Presence show={!!actionError} className="px-4 pt-3">
						{/* `lastActionError` rather than `actionError`, so the text is
						    still there to read while the notice fades. Reading the live
						    value would empty the box on the first frame of the exit and
						    animate a blank rectangle away. */}
						<Notice
							title={splitTechnical(lastActionError).message}
							detail="The vault is unchanged."
							{...(splitTechnical(lastActionError).technical
								? { technical: splitTechnical(lastActionError).technical }
								: {})}
						/>
					</Presence>

					{editing ? (
						<EntryEditor
							entry={entry}
							onSaved={async () => {
								// Stored before the editor closes. Closing first and
								// writing after would put the word Saved on screen while
								// the write could still fail.
								await onSaveVault();
								onEditing(false);
								setReload((n) => n + 1);
								onChanged();
							}}
							onCancel={() => onEditing(false)}
						/>
					) : (
						<div className="flex-1 overflow-y-auto p-4">
							<TotpRow uuid={entry.uuid} />

							{entry.fields.map((f) => (
								<FieldRow key={f.key} uuid={entry.uuid} field={f} />
							))}

							{(entry.attachments.length > 0 || writable) && (
								<div className="mt-5">
									<GroupHeading>Attachments</GroupHeading>
									{entry.attachments.map((a) => (
										<AttachmentRow
											key={a.name}
											uuid={entry.uuid}
											name={a.name}
											size={a.size}
											{...(writable
												? { onRemoved: () => setReload((n) => n + 1) }
												: {})}
										/>
									))}

									{/* Attaching was the missing half: files could be taken
									    out and never put in, which makes the feature useful
									    only for vaults filled somewhere else. */}
									{writable && (
										<button
											type="button"
											onClick={() => void attach()}
											disabled={attaching}
											className="btn btn-quiet -mx-2 mt-1 cursor-pointer rounded-[6px] px-2 py-1"
											style={{ color: "var(--window-active-text-fg)" }}
										>
											{attaching ? "Attaching…" : "Attach a file…"}
										</button>
									)}
								</div>
							)}

							<div className="mt-5">
								{/* A date a person would say. `6/3/2026, 10:17:46 AM` is a
								    machine's answer: nobody needs the second a password was
								    edited, and 6/3 means two different days depending on who
								    is reading. The full timestamp stays in the tooltip. */}
								<p
									className="text-[var(--window-fg)]"
									title={new Date(entry.times.modified).toLocaleString()}
								>
									Edited {humanDate(entry.times.modified)}
								</p>
								{/* Openable. This was a count and nothing else, which is a
								    tease: the reason anyone reads it is that they changed a
								    password, the site did not take it, and the old one is
								    now the only way back in. */}
								{entry.historyCount > 0 && (
									<EntryHistory
										uuid={entry.uuid}
										count={entry.historyCount}
										{...(writable ? { onRestore: restore } : {})}
									/>
								)}
							</div>

							{!writable && (
								<p className="mt-5 text-[var(--window-sub-text-fg)]">
									KDBX 3.1 is read-only in Vardo.
								</p>
							)}
						</div>
					)}
				</div>
			)}
		</section>
	);
}

/**
 * Shown above the fields, because a one-time code is the thing you came for
 * when an entry has one — burying it under Notes would mean scrolling past the
 * password every time.
 */
