import { useEffect, useRef, useState } from "react";

import { humanDate, identityOf, initialOf, tintFor } from "../lib/identity";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { EntryDetail, HistoryVersion, Uuid } from "../ipc";
import { EntryEditor } from "./EntryEditor";
import { Notice, splitTechnical } from "./Notice";
import { Presence } from "./Presence";
import { TotpCode } from "./TotpCode";

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
	/** Entries already tried, so a site with no icon is asked once per session. */
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
			if (next && !next.iconPng && writable && !tried.current.has(uuid)) {
				tried.current.add(uuid);
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
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [uuid, reload]);

	useEffect(() => setConfirmingDelete(false), [uuid]);

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
						animation:
							"fade-in var(--duration-universal) var(--ease-out-circ)",
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
								background: tintFor(
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
									style={{ width: "76%", height: "76%", objectFit: "contain" }}
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

							{entry.attachments.length > 0 && (
								<div className="mt-5">
									<GroupHeading>Attachments</GroupHeading>
									{entry.attachments.map((a) => (
										<AttachmentRow
											key={a.name}
											uuid={entry.uuid}
											name={a.name}
											size={a.size}
										/>
									))}
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
									<EntryHistory uuid={entry.uuid} count={entry.historyCount} />
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
function TotpRow({ uuid }: { uuid: Uuid }) {
	const [present, setPresent] = useState(false);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const code = await (await ipc()).totp(uuid);
				if (!cancelled) setPresent(code !== null);
			} catch {
				if (!cancelled) setPresent(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [uuid]);

	if (!present) return null;

	return (
		// The rule sits below its own padding and above a margin, so the first
		// field's highlight has somewhere to be. Flush against the border it read
		// as the hover box colliding with the line rather than sitting under it.
		<div
			className="mb-3 flex items-start gap-2 pt-1 pb-3"
			style={{ borderBottom: "1px solid var(--shadow-fg)" }}
		>
			<div className="min-w-0 flex-1">
				<GroupHeading>One-time code</GroupHeading>
				<TotpCode
					uuid={uuid}
					onCopy={() => {
						// Copying the code goes through the same Rust clipboard path
						// as a password, so it is auto-cleared on the same schedule.
						void (async () => {
							try {
								await (await ipc()).copyField(uuid, "otp");
							} catch {
								/* nothing to copy */
							}
						})();
					}}
				/>
			</div>
		</div>
	);
}

/**
 * The label under a value.
 *
 * Under, not over, and not shouted in capitals. Telegram's info panel puts the
 * value first because that is what you came to read - the label only tells you
 * what you are already looking at. Uppercase tracking-wide headers are the iOS
 * settings idiom, and using them here made every field look like a form.
 */
/**
 * A heading above a group of rows, as opposed to the label under one value.
 * Still not shouted: weight and colour carry it.
 */
/**
 * One attachment, with a way to get it out.
 *
 * It used to be a filename and a byte count and nothing else. People put
 * recovery codes and SSH keys in here precisely because they matter, and a
 * vault that can show you the name of the thing you need but never the thing
 * itself is somewhere data goes to be lost.
 */
function AttachmentRow({
	uuid,
	name,
	size,
}: {
	uuid: Uuid;
	name: string;
	size: number | null;
}) {
	const [state, setState] = useState<"idle" | "saving" | "done" | "failed">(
		"idle",
	);

	async function save() {
		setState("saving");
		try {
			const api = await ipc();
			const dest = await api.pickSavePath(name);
			if (!dest) {
				setState("idle");
				return;
			}
			await api.saveAttachment(uuid, name, dest);
			setState("done");
		} catch {
			setState("failed");
		}
	}

	return (
		<div className="group flex items-center gap-2 py-1.5">
			<svg
				width="16"
				height="16"
				viewBox="0 0 24 24"
				fill="none"
				stroke="var(--window-sub-text-fg)"
				strokeWidth="1.8"
				strokeLinecap="round"
				aria-hidden
				className="shrink-0"
			>
				<path d="M14 3v5h5M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
			</svg>
			<span className="min-w-0 flex-1 truncate">{name}</span>
			{size !== null && (
				<span className="shrink-0 tabular-nums text-[var(--window-sub-text-fg)] text-[12px]">
					{formatBytes(size)}
				</span>
			)}
			<button
				type="button"
				onClick={() => void save()}
				disabled={state === "saving"}
				className="btn btn-quiet shrink-0 cursor-pointer rounded-[6px] px-2 py-0.5 opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
				style={{
					color:
						state === "failed"
							? "var(--attention-button-fg)"
							: "var(--window-active-text-fg)",
					transitionDuration: "var(--duration-universal)",
					opacity: state === "idle" ? undefined : 1,
				}}
			>
				{state === "saving"
					? "Saving\u2026"
					: state === "done"
						? "Saved"
						: state === "failed"
							? "Failed"
							: "Save\u2026"}
			</button>
		</div>
	);
}

function formatBytes(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/**
 * Earlier versions of this entry.
 *
 * Collapsed by default and loaded only when opened, because history holds old
 * passwords and there is no reason to have them in memory while someone is
 * looking at something else.
 *
 * Copy, not restore. Restoring would be another write and another way to lose
 * the current password by accident; what people actually need is to paste the
 * old one into a site that rejected the change.
 */
function EntryHistory({ uuid, count }: { uuid: Uuid; count: number }) {
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

function GroupHeading({ children }: { children: React.ReactNode }) {
	return (
		<h3
			className="mb-1 font-semibold"
			style={{ color: "var(--window-active-text-fg)", fontSize: "13px" }}
		>
			{children}
		</h3>
	);
}

function FieldLabel({ children }: { children: React.ReactNode }) {
	return (
		<div style={{ color: "var(--window-sub-text-fg)", fontSize: "12px" }}>
			{children}
		</div>
	);
}

function EmptyState() {
	return (
		<div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
			{/* DESIGN.md: empty states say something useful, and no illustrations. */}
			<p className="text-[var(--window-sub-text-fg)]">
				Select an entry to see its details.
			</p>
			<p className="text-[var(--window-sub-text-fg)]">
				<span className="inline-flex flex-wrap justify-center gap-x-4 gap-y-1">
					<span>
						<Kbd>Ctrl</Kbd> <Kbd>F</Kbd> to search
					</span>
					<span>
						<Kbd>↑</Kbd> <Kbd>↓</Kbd> to move
					</span>
					<span>
						<Kbd>Ctrl</Kbd> <Kbd>L</Kbd> to lock
					</span>
				</span>
			</p>
		</div>
	);
}

function Kbd({ children }: { children: React.ReactNode }) {
	return (
		<kbd
			className="rounded-[3px] px-1 py-0.5 font-sans text-[11px]"
			style={{
				background: "var(--window-bg-over)",
				color: "var(--window-sub-text-fg)",
			}}
		>
			{children}
		</kbd>
	);
}

function FieldRow({
	uuid,
	field,
}: {
	uuid: Uuid;
	field: EntryDetail["fields"][number];
}) {
	const [revealed, setRevealed] = useState<string | null>(null);
	const [copiedFor, setCopiedFor] = useState<number | null>(null);

	// A revealed secret must not outlive the entry it belongs to.
	useEffect(() => {
		setRevealed(null);
	}, [uuid]);

	useEffect(() => {
		if (copiedFor === null) return;
		const t = window.setTimeout(() => setCopiedFor(null), copiedFor * 1000);
		return () => window.clearTimeout(t);
	}, [copiedFor]);

	if (field.empty && !field.protected) return null;

	async function copy() {
		// The secret goes Rust → clipboard. It never becomes a JS string here.
		const { clearAfterSeconds } = await (await ipc()).copyField(
			uuid,
			field.key,
		);
		setCopiedFor(clearAfterSeconds);
	}

	async function toggleReveal() {
		if (revealed !== null) {
			setRevealed(null);
			return;
		}
		setRevealed(await (await ipc()).revealField(uuid, field.key));
	}

	const shown = field.protected
		? (revealed ?? "••••••••••••")
		: (field.value ?? "");

	// Hover is handled by the stylesheet rather than by two mouse handlers
	// writing inline styles. Those worked, and were a small state machine per
	// row that gets it wrong whenever a pointer leaves the window mid-hover.
	return (
		// `-mx-2 px-2` rather than plain padding. Padding alone pushed the value
		// eight pixels right of every heading and of the footer, so the column had
		// two left edges depending on whether a line happened to be hoverable.
		// Pulling the box out by the same amount it pads back in keeps the text on
		// one edge and lets the highlight extend into the gutter.
		//
		// `my-0.5` because the box was landing flush against the countdown ring
		// above it, which read as two things touching rather than one thing
		// lighting up.
		<div className="group btn btn-quiet -mx-2 my-0.5 flex items-center gap-2 rounded-[6px] px-2 py-2">
			{/* The whole row copies, the way tapping a phone number in Telegram
			    does. The buttons stay for discoverability and for anyone using a
			    keyboard, but the common action does not require aiming. */}
			<button
				type="button"
				onClick={copy}
				title={`Copy ${field.key.toLowerCase()}`}
				// No hover of its own: the row it sits in already has one, and two
				// nested hover backgrounds make the inner one look like a
				// separate control floating on the row.
				className="min-w-0 flex-1 cursor-pointer text-left"
			>
				<div
					className={`selectable break-words ${field.protected ? "secret" : ""}`}
					style={{ color: "var(--window-fg)", whiteSpace: "pre-wrap" }}
				>
					{shown}
				</div>
				<FieldLabel>
					{copiedFor === null
						? field.key
						: `Copied. Clipboard clears in ${copiedFor}s`}
				</FieldLabel>
			</button>

			<div className="flex shrink-0 items-center gap-0.5">
				{field.protected && (
					<GlyphButton
						label={revealed !== null ? "Hide" : "Show"}
						onClick={toggleReveal}
						pressed={revealed !== null}
					>
						{revealed !== null ? <EyeOffIcon /> : <EyeIcon />}
					</GlyphButton>
				)}
				<GlyphButton label={`Copy ${field.key}`} onClick={copy} pressed={false}>
					<CopyIcon />
				</GlyphButton>
			</div>
		</div>
	);
}

/**
 * A square icon button that only appears on hover or focus.
 *
 * "Reveal" and "Copy" as words made every row look like a toolbar and pushed
 * the value - the thing you came for - into a narrow column.
 */
function GlyphButton({
	label,
	onClick,
	pressed,
	children,
}: {
	label: string;
	onClick: () => void;
	pressed: boolean;
	children: React.ReactNode;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			aria-pressed={pressed}
			aria-label={label}
			title={label}
			className="btn btn-quiet flex h-7 w-7 cursor-pointer items-center justify-center rounded-[6px] opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
			style={{
				color: pressed
					? "var(--window-active-text-fg)"
					: "var(--window-sub-text-fg)",
				transitionDuration: "var(--duration-universal)",
			}}
		>
			{children}
		</button>
	);
}

function EyeIcon() {
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.8"
			aria-hidden
		>
			<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7-10-7-10-7z" />
			<circle cx="12" cy="12" r="3" />
		</svg>
	);
}

function EyeOffIcon() {
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.8"
			aria-hidden
		>
			<path d="M2 12s3.6-7 10-7c2 0 3.7.7 5.1 1.6M22 12s-3.6 7-10 7c-2 0-3.7-.7-5.1-1.6" />
			<path d="M4 4l16 16" />
		</svg>
	);
}

function CopyIcon() {
	return (
		<svg
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.8"
			aria-hidden
		>
			<rect x="9" y="9" width="11" height="11" rx="2" />
			<path d="M5 15V5a2 2 0 0 1 2-2h10" />
		</svg>
	);
}
