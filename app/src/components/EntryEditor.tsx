import { useEffect, useMemo, useState } from "react";

import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { EntryDetail, FieldEdit } from "../ipc";
import { PasswordGenerator } from "./PasswordGenerator";

interface Props {
	entry: EntryDetail;
	onSaved: () => void;
	onCancel: () => void;
}

const STANDARD = ["Title", "UserName", "Password", "URL", "Notes"];

/**
 * Editing form for one entry.
 *
 * # Why the password starts empty rather than revealed
 *
 * A protected field arrives without its value (see `ipc/types.ts`), so there is
 * nothing to prefill. That is deliberate rather than awkward: opening an editor
 * should not pull every secret into the WebView. The field shows a placeholder
 * meaning "unchanged", and only a value the user actually types is submitted.
 */
export function EntryEditor({ entry, onSaved, onCancel }: Props) {
	const initial = useMemo(() => {
		const map = new Map<string, string>();
		for (const f of entry.fields) {
			if (!f.protected) map.set(f.key, f.value ?? "");
		}
		map.set("Title", entry.title);
		return map;
	}, [entry]);

	const [values, setValues] = useState<Map<string, string>>(
		() => new Map(initial),
	);
	const [password, setPassword] = useState<string | null>(null);
	const [showGenerator, setShowGenerator] = useState(false);
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);

	// A different entry means a different form; never carry a typed password
	// across.
	useEffect(() => {
		setValues(new Map(initial));
		setPassword(null);
		setShowGenerator(false);
		setError(null);
	}, [initial]);

	// And never let one outlive the editor.
	useEffect(() => () => setPassword(null), []);

	const customKeys = entry.fields
		.map((f) => f.key)
		.filter((k) => !STANDARD.includes(k));

	const set = (key: string, value: string) =>
		setValues((m) => new Map(m).set(key, value));

	async function submit(e: React.FormEvent) {
		e.preventDefault();
		if (saving) return;
		setSaving(true);
		setError(null);

		const edits: FieldEdit[] = [];
		for (const [key, value] of values) {
			if (initial.get(key) !== value) {
				edits.push({ key, value, protected: false });
			}
		}
		// Only submit the password if one was actually typed. An untouched field
		// must not overwrite the stored secret with an empty string.
		if (password !== null) {
			edits.push({ key: "Password", value: password, protected: true });
		}

		try {
			const api = await ipc();
			if (edits.length > 0) await api.updateEntry(entry.uuid, edits);
			setPassword(null);
			onSaved();
		} catch (err) {
			setError(errorText(err));
		} finally {
			setSaving(false);
		}
	}

	return (
		<form onSubmit={submit} className="flex h-full flex-col">
			<div className="flex-1 overflow-y-auto p-4">
				<Row label="Title">
					<Input
						value={values.get("Title") ?? ""}
						onChange={(v) => set("Title", v)}
						autoFocus
					/>
				</Row>

				<Row label="Username">
					<Input
						value={values.get("UserName") ?? ""}
						onChange={(v) => set("UserName", v)}
					/>
				</Row>

				<Row label="Password">
					<div className="flex items-center gap-2">
						<input
							type="password"
							value={password ?? ""}
							placeholder={password === null ? "Unchanged" : ""}
							autoComplete="off"
							spellCheck={false}
							onChange={(e) => setPassword(e.target.value)}
							className="secret min-w-0 flex-1 rounded-[4px] px-2 py-1.5 outline-none"
							style={{
								background: "var(--window-bg-over)",
								color: "var(--window-fg)",
								border: "1px solid transparent",
							}}
						/>
						<button
							type="button"
							onClick={() => setShowGenerator((s) => !s)}
							className="shrink-0 cursor-pointer rounded-[6px] px-2 py-1"
							style={{ color: "var(--window-active-text-fg)" }}
						>
							Generate
						</button>
					</div>
					{showGenerator && (
						<div className="mt-2">
							<PasswordGenerator
								onUse={(pw) => {
									setPassword(pw);
									setShowGenerator(false);
								}}
								onClose={() => setShowGenerator(false)}
							/>
						</div>
					)}
				</Row>

				<Row label="URL">
					<Input
						value={values.get("URL") ?? ""}
						onChange={(v) => set("URL", v)}
					/>
				</Row>

				<Row label="Notes">
					<textarea
						value={values.get("Notes") ?? ""}
						onChange={(e) => set("Notes", e.target.value)}
						rows={4}
						className="w-full resize-y rounded-[4px] px-2 py-1.5 outline-none"
						style={{
							background: "var(--window-bg-over)",
							color: "var(--window-fg)",
							border: "1px solid transparent",
							fontFamily: "inherit",
							fontSize: "inherit",
						}}
					/>
				</Row>

				{customKeys.map((key) => (
					<Row key={key} label={key}>
						<Input
							value={values.get(key) ?? ""}
							onChange={(v) => set(key, v)}
						/>
					</Row>
				))}

				{error && (
					<p className="mt-3" style={{ color: "var(--attention-button-fg)" }}>
						{error}
					</p>
				)}
			</div>

			<div
				className="flex items-center gap-2 px-4 py-3"
				style={{ borderTop: "1px solid var(--shadow-fg)" }}
			>
				<button
					type="submit"
					disabled={saving}
					className="cursor-pointer rounded-[6px] px-3 py-1.5 font-semibold disabled:opacity-45"
					style={{
						background: "var(--active-button-bg)",
						color: "var(--window-fg-active)",
					}}
				>
					{saving ? "Applying…" : "Apply"}
				</button>
				<button
					type="button"
					onClick={onCancel}
					className="cursor-pointer rounded-[6px] px-3 py-1.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Cancel
				</button>
				{/* "to disk" was wrong for half of them: a vault in Drive has no
				    disk here at all. The distinction that matters is the one this
				    now makes - Apply changes the open vault, saving writes it back
				    to wherever it came from. */}
				<span className="ml-auto text-[var(--window-sub-text-fg)] text-[12px]">
					Apply keeps this in the open vault. Save stores it.
				</span>
			</div>
		</form>
	);
}

function Row({
	label,
	children,
}: {
	label: string;
	children: React.ReactNode;
}) {
	return (
		<div className="mb-3">
			{/* Not shouted. Uppercase tracking-wide headers are the iOS settings
			    idiom, and next to a detail panel that had already dropped them
			    this made editing look like a different application. */}
			<div
				className="mb-1"
				style={{
					color: "var(--window-sub-text-fg)",
					fontSize: "12px",
				}}
			>
				{label}
			</div>
			{children}
		</div>
	);
}

function Input({
	value,
	onChange,
	autoFocus,
}: {
	value: string;
	onChange: (v: string) => void;
	autoFocus?: boolean;
}) {
	return (
		<input
			type="text"
			value={value}
			spellCheck={false}
			// biome-ignore lint/a11y/noAutofocus: the editor is opened by an
			// explicit user action, and focusing the first field is what every
			// desktop form does.
			autoFocus={autoFocus}
			onChange={(e) => onChange(e.target.value)}
			className="w-full rounded-[4px] px-2 py-1.5 outline-none"
			style={{
				background: "var(--window-bg-over)",
				color: "var(--window-fg)",
				border: "1px solid transparent",
			}}
		/>
	);
}
