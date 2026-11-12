import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import type { EntryDetail, Uuid } from "../ipc";

export function GroupHeading({ children }: { children: React.ReactNode }) {
	return (
		<h3
			className="mb-1 font-semibold"
			style={{ color: "var(--window-active-text-fg)", fontSize: "13px" }}
		>
			{children}
		</h3>
	);
}

export function FieldLabel({ children }: { children: React.ReactNode }) {
	return (
		<div style={{ color: "var(--window-sub-text-fg)", fontSize: "12px" }}>
			{children}
		</div>
	);
}

export function EmptyState() {
	return (
		<div className="flex h-full flex-col items-center justify-center gap-1 px-6 text-center">
			{/* Empty states say something useful, and carry no illustration. */}
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

export function Kbd({ children }: { children: React.ReactNode }) {
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

/**
 * What a person calls a field, as opposed to what KeePass calls it.
 *
 * The detail view printed the raw key, so it said "UserName" directly under
 * an editor that said "Username", and a screen reader announced "Copy User
 * Name". Only KeePass's own standard key needs translating; custom fields are
 * shown exactly as the person typed them, because that spelling was theirs.
 */
export function labelOf(key: string): string {
	return key === "UserName" ? "Username" : key;
}

export function FieldRow({
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
	const label = labelOf(field.key);

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
				title={`Copy ${label.toLowerCase()}`}
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
						? label
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
				<GlyphButton label={`Copy ${label}`} onClick={copy} pressed={false}>
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
export function GlyphButton({
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

export function EyeIcon() {
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

export function EyeOffIcon() {
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

export function CopyIcon() {
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
