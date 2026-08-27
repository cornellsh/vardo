import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import type { EntryDetail, Uuid } from "../ipc";
import { EntryEditor } from "./EntryEditor";
import { TotpCode } from "./TotpCode";

interface Props {
	uuid: Uuid | null;
	writable: boolean;
	/** Called after any change, so the list and dirty state refresh. */
	onChanged: () => void;
	/** Opened straight into edit mode, for a freshly created entry. */
	editing: boolean;
	onEditing: (editing: boolean) => void;
}

export function EntryDetailPane({
	uuid,
	writable,
	onChanged,
	editing,
	onEditing,
}: Props) {
	const [entry, setEntry] = useState<EntryDetail | null>(null);
	const [reload, setReload] = useState(0);
	const [confirmingDelete, setConfirmingDelete] = useState(false);

	useEffect(() => {
		if (!uuid) {
			setEntry(null);
			return;
		}
		let cancelled = false;
		void (async () => {
			const next = await (await ipc()).getEntry(uuid);
			if (!cancelled) setEntry(next);
		})();
		return () => {
			cancelled = true;
		};
	}, [uuid, reload]);

	useEffect(() => setConfirmingDelete(false), [uuid]);

	async function remove() {
		if (!uuid) return;
		await (await ipc()).deleteEntry(uuid);
		setConfirmingDelete(false);
		onChanged();
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
				<>
					<header
						className="flex items-center px-4"
						style={{
							height: "54px",
							borderBottom: "1px solid var(--shadow-fg)",
						}}
					>
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
									className="cursor-pointer rounded-[6px] px-2 py-1"
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
											className="cursor-pointer rounded-[6px] px-2 py-1 font-semibold"
											style={{ color: "var(--attention-button-fg)" }}
										>
											Confirm
										</button>
										<button
											type="button"
											onClick={() => setConfirmingDelete(false)}
											className="cursor-pointer rounded-[6px] px-2 py-1"
											style={{ color: "var(--window-sub-text-fg)" }}
										>
											Keep
										</button>
									</>
								) : (
									<button
										type="button"
										onClick={() => setConfirmingDelete(true)}
										className="cursor-pointer rounded-[6px] px-2 py-1"
										style={{ color: "var(--attention-button-fg)" }}
									>
										Delete
									</button>
								)}
							</div>
						)}
					</header>

					{editing ? (
						<EntryEditor
							entry={entry}
							onSaved={() => {
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
									<SectionLabel>Attachments</SectionLabel>
									{entry.attachments.map((a) => (
										<div
											key={a.name}
											className="flex items-center gap-2 py-1.5"
										>
											<span className="truncate">{a.name}</span>
											{a.size !== null && (
												<span className="tabular-nums text-[var(--window-sub-text-fg)] text-[12px]">
													{a.size} B
												</span>
											)}
										</div>
									))}
								</div>
							)}

							<div className="mt-5">
								<SectionLabel>Modified</SectionLabel>
								<p className="text-[var(--window-sub-text-fg)]">
									{new Date(entry.times.modified).toLocaleString()}
								</p>
								{entry.historyCount > 0 && (
									<p className="text-[var(--window-sub-text-fg)]">
										{entry.historyCount === 1
											? "1 earlier version"
											: `${entry.historyCount} earlier versions`}
									</p>
								)}
							</div>

							{!writable && (
								<p className="mt-5 text-[var(--window-sub-text-fg)]">
									This database is KDBX 3.1, which Vardo can read but not write.
								</p>
							)}
						</div>
					)}
				</>
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
		<div
			className="flex items-start gap-2 py-2"
			style={{ borderBottom: "1px solid var(--shadow-fg)" }}
		>
			<div className="min-w-0 flex-1">
				<SectionLabel>One-time code</SectionLabel>
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

function SectionLabel({ children }: { children: React.ReactNode }) {
	return (
		<h3
			className="mb-1 font-semibold uppercase"
			style={{
				color: "var(--window-sub-text-fg)",
				fontSize: "11px",
				letterSpacing: "0.04em",
			}}
		>
			{children}
		</h3>
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

	return (
		<div
			className="group flex items-start gap-2 py-2"
			style={{ borderBottom: "1px solid var(--shadow-fg)" }}
		>
			<div className="min-w-0 flex-1">
				<SectionLabel>{field.key}</SectionLabel>
				<div
					className={`selectable break-words ${field.protected ? "secret" : ""}`}
					style={{ color: "var(--window-fg)", whiteSpace: "pre-wrap" }}
				>
					{shown}
				</div>
			</div>

			<div className="flex shrink-0 items-center gap-1">
				{field.protected && (
					<IconButton
						label={revealed !== null ? "Hide" : "Reveal"}
						onClick={toggleReveal}
						pressed={revealed !== null}
					/>
				)}
				<button
					type="button"
					onClick={copy}
					className="cursor-pointer rounded-[6px] px-2 py-1"
					style={{ color: "var(--window-active-text-fg)" }}
				>
					{copiedFor === null ? "Copy" : `Clears in ${copiedFor}s`}
				</button>
			</div>
		</div>
	);
}

function IconButton({
	label,
	onClick,
	pressed,
}: {
	label: string;
	onClick: () => void;
	pressed: boolean;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			aria-pressed={pressed}
			className="cursor-pointer rounded-[6px] px-2 py-1"
			style={{ color: "var(--window-active-text-fg)" }}
		>
			{label}
		</button>
	);
}
