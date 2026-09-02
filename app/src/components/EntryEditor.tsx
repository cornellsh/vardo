import { useEffect, useMemo, useState } from "react";

import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { EntryDetail, FieldEdit } from "../ipc";
import { PasswordGenerator } from "./PasswordGenerator";
import { Presence } from "./Presence";

interface Props {
	entry: EntryDetail;
	/**
	 * Commits the edit and stores the vault. Awaited, so the button stays busy
	 * until the bytes are really written and the word Save is not a lie.
	 */
	onSaved: () => Promise<void> | void;
	onCancel: () => void;
}

const STANDARD = ["Title", "UserName", "Password", "URL", "Notes"];

/**
 * Where the three KeePass clients each put a one-time code seed.
 *
 * Reading all of them and writing the first is what keeps a vault usable in
 * KeePassXC and KeePassium: inventing a fourth convention would make Vardo the
 * only program that could see its own codes.
 */
const OTP_KEYS = ["otp", "TOTP Seed", "TimeOtp-Secret-Base32"];

/**
 * `yyyy-mm-dd` in local time, for a native date input.
 *
 * Not `toISOString().slice(0, 10)`, which converts to UTC first and therefore
 * shows the previous day to anyone east of Greenwich during their evening. The
 * bug it causes is a date that is right when you set it and wrong when you
 * reopen it, which reads as the save having failed.
 */
function toDateInput(date: Date): string {
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

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

	// Comma-separated in the box, an array on the wire. KDBX itself joins with
	// semicolons, which nobody types on purpose.
	const initialTags = entry.tags.join(", ");
	const [tags, setTags] = useState(initialTags);

	// `yyyy-mm-dd` for the native date input, built from local parts rather
	// than `toISOString`, which converts to UTC first and lands on the previous
	// day for anyone east of Greenwich in the evening.
	const initialExpiry = entry.times.expires
		? toDateInput(new Date(entry.times.expires))
		: "";
	const [expiry, setExpiry] = useState(initialExpiry);

	/**
	 * Which key a seed gets written to.
	 *
	 * Whichever one the entry already uses, so editing a vault made in
	 * KeePassXC does not quietly leave a second seed under a different name
	 * that some other client will then prefer. `otp` for a new one, because it
	 * is the convention every current client reads.
	 */
	const otpKey =
		entry.fields.map((f) => f.key).find((k) => OTP_KEYS.includes(k)) ?? "otp";
	const [saving, setSaving] = useState(false);
	const [error, setError] = useState<string | null>(null);

	// A different entry means a different form; never carry a typed password
	// across.
	useEffect(() => {
		setValues(new Map(initial));
		setPassword(null);
		setShowGenerator(false);
		setError(null);
		setTags(initialTags);
		setExpiry(initialExpiry);
		setRemoved(new Set());
	}, [initial, initialTags, initialExpiry]);

	// And never let one outlive the editor.
	useEffect(() => () => setPassword(null), []);

	/**
	 * Custom fields, including ones added in this session.
	 *
	 * Read from `values` rather than from the entry, so a field added here shows
	 * up before it has been saved. Reading the entry meant a new field vanished
	 * the moment you typed its name, which is the sort of thing that makes
	 * people think they broke something.
	 *
	 * `otp` is excluded because it has a row of its own: a base32 seed in a
	 * generic text box is not a one-time code, and treating it as an ordinary
	 * field means nobody can tell whether they typed it correctly until the
	 * next time they need to log in.
	 */
	const customKeys = [...values.keys()].filter(
		(k) => !STANDARD.includes(k) && !OTP_KEYS.includes(k),
	);

	const set = (key: string, value: string) =>
		setValues((m) => new Map(m).set(key, value));

	/** Marked for removal on save, rather than dropped immediately. */
	const [removed, setRemoved] = useState<Set<string>>(new Set());

	function remove(key: string) {
		setRemoved((r) => new Set(r).add(key));
		setValues((m) => {
			const next = new Map(m);
			next.delete(key);
			return next;
		});
	}

	function addField(key: string) {
		const name = key.trim();
		if (!name) return;
		// Reusing a standard key would put two boxes on screen writing to one
		// field, with the last one saved winning silently.
		if (STANDARD.includes(name) || values.has(name)) return;
		setRemoved((r) => {
			const next = new Set(r);
			next.delete(name);
			return next;
		});
		set(name, "");
	}

	async function submit(e: React.FormEvent) {
		e.preventDefault();
		if (saving) return;
		setSaving(true);
		setError(null);

		const edits: FieldEdit[] = [];
		for (const [key, value] of values) {
			if (initial.get(key) !== value) {
				// The one-time code seed is protected like a password, because it
				// is one: anyone holding it can generate every future code, which
				// makes it worth more than any single code it produces.
				edits.push({
					key,
					value,
					protected: OTP_KEYS.includes(key),
				});
			}
		}
		// A null value is a removal, which the core already understood; nothing
		// in the UI had ever sent one.
		for (const key of removed) {
			if (initial.has(key)) edits.push({ key, value: null, protected: false });
		}
		// Only submit the password if one was actually typed. An untouched field
		// must not overwrite the stored secret with an empty string.
		if (password !== null) {
			edits.push({ key: "Password", value: password, protected: true });
		}

		try {
			const api = await ipc();
			if (edits.length > 0) await api.updateEntry(entry.uuid, edits);

			if (tags !== initialTags) {
				await api.setTags(
					entry.uuid,
					tags
						.split(",")
						.map((t) => t.trim())
						.filter(Boolean),
				);
			}
			if (expiry !== initialExpiry) {
				// Midday local, not midnight. An expiry stored at 00:00 is a day
				// earlier than intended for anyone west of UTC once it is read
				// back, and the resulting off-by-one is invisible until an entry
				// reports itself expired the day before it should.
				const at = expiry ? Date.parse(`${expiry}T12:00:00`) : null;
				await api.setExpiry(
					entry.uuid,
					at === null || Number.isNaN(at) ? null : Math.floor(at / 1000),
				);
			}
			setPassword(null);
			// Save means saved. This used to say Apply, commit to the open vault,
			// and leave a bar demanding a second, differently named action - two
			// verbs for one intention, where the first sounded final and was not.
			// The split existed because re-encrypting costs a second or two, which
			// is a fact about Argon2 and not something a person should have to
			// hold in their head while typing a password in.
			await onSaved();
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
							className="btn btn-quiet shrink-0 cursor-pointer rounded-[6px] px-2 py-1"
							style={{ color: "var(--window-active-text-fg)" }}
						>
							Generate
						</button>
					</div>
					<Presence show={showGenerator} className="mt-2">
						<PasswordGenerator
							onUse={(pw) => {
								setPassword(pw);
								setShowGenerator(false);
							}}
							onClose={() => setShowGenerator(false)}
						/>
					</Presence>
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

				{/* The one-time code, which could be read and never written. A
				    password manager that shows you a code but cannot be given one is
				    only useful for accounts you set up somewhere else first. */}
				<Row label="One-time code">
					<Input
						value={values.get(otpKey) ?? ""}
						onChange={(v) => set(otpKey, v.trim())}
						placeholder="otpauth:// link, or the base32 secret"
					/>
					{/* Checked as it is typed. A mistyped seed produces codes that
					    look perfectly plausible and are simply wrong, and you find
					    out at the worst possible moment - locked out, in a hurry,
					    with no way to tell whether the seed or the clock is at
					    fault. */}
					<OtpCheck value={values.get(otpKey) ?? ""} />
				</Row>

				{customKeys.map((key) => (
					<Row key={key} label={key} onRemove={() => remove(key)}>
						<Input
							value={values.get(key) ?? ""}
							onChange={(v) => set(key, v)}
						/>
					</Row>
				))}

				{/* Tags and expiry are not fields - KDBX keeps them as their own
				    elements - so they are set by their own calls on save rather than
				    smuggled through the field list. Both have been readable since
				    the first version and editable in nothing. */}
				<Row label="Tags">
					<Input
						value={tags}
						onChange={setTags}
						placeholder="Separated by commas"
					/>
				</Row>

				<Row label="Expires">
					<div className="flex items-center gap-2">
						<input
							type="date"
							value={expiry}
							onChange={(e) => setExpiry(e.target.value)}
							className="min-w-0 flex-1 rounded-[4px] px-2 py-1.5 outline-none"
							style={{
								background: "var(--window-bg-over)",
								color: "var(--window-fg)",
								border: "1px solid transparent",
								colorScheme: "dark light",
							}}
						/>
						{expiry && (
							<button
								type="button"
								onClick={() => setExpiry("")}
								className="btn btn-quiet shrink-0 cursor-pointer rounded-[6px] px-2 py-1"
								style={{ color: "var(--window-sub-text-fg)" }}
							>
								Never
							</button>
						)}
					</div>
				</Row>

				<AddField onAdd={addField} existing={[...values.keys()]} />

				<Presence show={!!error} className="mt-3">
					<p style={{ color: "var(--attention-button-fg)" }}>{error}</p>
				</Presence>
			</div>

			<div
				className="flex items-center gap-2 px-4 py-3"
				style={{ borderTop: "1px solid var(--shadow-fg)" }}
			>
				<button
					type="submit"
					disabled={saving}
					className="btn btn-accent cursor-pointer rounded-[6px] px-3 py-1.5 font-semibold"
					style={{
						color: "var(--window-fg-active)",
					}}
				>
					{saving ? "Saving…" : "Save"}
				</button>
				<button
					type="button"
					onClick={onCancel}
					className="btn btn-quiet cursor-pointer rounded-[6px] px-3 py-1.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Cancel
				</button>
			</div>
		</form>
	);
}

function Row({
	label,
	children,
	onRemove,
}: {
	label: string;
	children: React.ReactNode;
	/** Present only for fields the user added and may take away again. */
	onRemove?: (() => void) | undefined;
}) {
	return (
		<div className="group mb-3">
			{/* Not shouted. Uppercase tracking-wide headers are the iOS settings
			    idiom, and next to a detail panel that had already dropped them
			    this made editing look like a different application. */}
			<div className="mb-1 flex items-center gap-2">
				<span
					style={{
						color: "var(--window-sub-text-fg)",
						fontSize: "12px",
					}}
				>
					{label}
				</span>
				{onRemove && (
					<button
						type="button"
						onClick={onRemove}
						aria-label={`Remove ${label}`}
						className="on-hover cursor-pointer"
						style={{
							color: "var(--attention-button-fg)",
							fontSize: "12px",
						}}
					>
						Remove
					</button>
				)}
			</div>
			{children}
		</div>
	);
}

/**
 * Adds a custom field.
 *
 * The core has always understood arbitrary keys - `set_field` creates whatever
 * it is given - and nothing in the interface ever offered one. Which made this
 * an editor that could only edit the five fields someone else had decided on,
 * while real vaults are full of PINs, account numbers, security answers and
 * recovery codes.
 */
function AddField({
	onAdd,
	existing,
}: {
	onAdd: (key: string) => void;
	existing: string[];
}) {
	const [name, setName] = useState("");
	const trimmed = name.trim();
	const duplicate = existing.includes(trimmed) || STANDARD.includes(trimmed);

	function commit() {
		if (!trimmed || duplicate) return;
		onAdd(trimmed);
		setName("");
	}

	return (
		<div className="mt-4 mb-2">
			<div className="flex items-center gap-2">
				<input
					type="text"
					value={name}
					spellCheck={false}
					placeholder="Add a field…"
					onChange={(e) => setName(e.target.value)}
					// Enter adds the field rather than submitting the form, which
					// would save the entry and close the editor while someone was
					// halfway through adding something to it.
					onKeyDown={(e) => {
						if (e.key === "Enter") {
							e.preventDefault();
							commit();
						}
					}}
					className="min-w-0 flex-1 rounded-[4px] px-2 py-1.5 outline-none"
					style={{
						background: "var(--window-bg-over)",
						color: "var(--window-fg)",
						border: "1px solid transparent",
					}}
				/>
				<button
					type="button"
					onClick={commit}
					disabled={!trimmed || duplicate}
					className="btn btn-quiet shrink-0 cursor-pointer rounded-[6px] px-3 py-1.5 font-semibold"
					style={{ color: "var(--window-active-text-fg)" }}
				>
					Add
				</button>
			</div>
			{/* Said before the button is pressed. Silently refusing would look
			    like the button was broken, and overwriting the existing field is
			    how you lose whatever was in it. */}
			<Presence show={!!trimmed && duplicate} className="mt-1">
				<p
					className="text-[12px]"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					There is already a field called {trimmed}.
				</p>
			</Presence>
		</div>
	);
}

/**
 * Tells you whether a one-time code seed is usable, as you type it.
 *
 * A mistyped seed produces codes that look perfectly plausible and are simply
 * wrong, and you find out at the worst possible moment: locked out, in a hurry,
 * with no way to tell whether the seed or the clock is at fault. The parser
 * that will read it back is right there, so it may as well answer now.
 */
function OtpCheck({ value }: { value: string }) {
	const [state, setState] = useState<"idle" | "ok" | "bad">("idle");

	useEffect(() => {
		const text = value.trim();
		if (!text) {
			setState("idle");
			return;
		}
		let cancelled = false;
		// Debounced, because this runs on every keystroke and a base32 seed is
		// long enough that checking each prefix would flash "not valid" through
		// most of the typing.
		const t = window.setTimeout(() => {
			void (async () => {
				try {
					const ok = await (await ipc()).checkTotpSeed(text);
					if (!cancelled) setState(ok ? "ok" : "bad");
				} catch {
					if (!cancelled) setState("bad");
				}
			})();
		}, 300);
		return () => {
			cancelled = true;
			window.clearTimeout(t);
		};
	}, [value]);

	return (
		<Presence show={state !== "idle"} className="mt-1">
			<p
				className="text-[12px]"
				style={{
					color:
						state === "ok"
							? "var(--window-sub-text-fg)"
							: "var(--attention-button-fg)",
				}}
			>
				{state === "ok"
					? "Codes will generate from this."
					: "Vardo cannot read this as a code seed."}
			</p>
		</Presence>
	);
}

function Input({
	value,
	onChange,
	autoFocus,
	placeholder,
}: {
	value: string;
	onChange: (v: string) => void;
	autoFocus?: boolean;
	placeholder?: string;
}) {
	return (
		<input
			type="text"
			value={value}
			spellCheck={false}
			{...(placeholder ? { placeholder } : {})}
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
