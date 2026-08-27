import { useEffect, useState } from "react";

import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { ConflictPolicy, SyncOutcome, SyncPreview } from "../ipc";

interface Props {
	onClose: () => void;
	/** Called after a successful sync, so the list reloads from the merged vault. */
	onMerged: () => void;
}

/**
 * The merge review.
 *
 * No prior art: KeePassXC merges without asking and only added a preview dialog
 * in PR #10173. Everything here follows from one rule — **nothing changes until
 * the user has seen what will change.** So this screen is reachable before any
 * write, `syncPreview` mutates nothing, and the conflict policy is an explicit
 * choice rather than a setting buried elsewhere.
 *
 * The other rule is that a conflict must explain *why* it cannot be decided.
 * "Conflict" on its own reads as a bug in the merge; "both devices changed this
 * in the same second" reads as the fact of life it actually is.
 */
export function MergeReview({ onClose, onMerged }: Props) {
	const [preview, setPreview] = useState<SyncPreview | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [policy, setPolicy] = useState<ConflictPolicy>("duplicate");
	const [applying, setApplying] = useState(false);
	const [outcome, setOutcome] = useState<SyncOutcome | null>(null);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const next = await (await ipc()).syncPreview();
				if (!cancelled) setPreview(next);
			} catch (err) {
				if (!cancelled) setError(errorText(err));
			}
		})();
		return () => {
			cancelled = true;
		};
	}, []);

	async function applyMerge() {
		setApplying(true);
		setError(null);
		try {
			const result = await (await ipc()).syncApply(policy);
			setOutcome(result);
			if (result.kind !== "needs-review") {
				// The plan described a merge that has now happened. Leaving it on
				// screen would show changes still pending that are already applied,
				// which is the one thing a review screen must never do.
				setPreview({
					remote: preview?.remote ?? "",
					firstSync: false,
					remoteUnchanged: true,
					changes: [],
					conflicts: [],
					warnings: [],
				});
				onMerged();
			}
		} catch (err) {
			setError(errorText(err));
		} finally {
			setApplying(false);
		}
	}

	const nothingToDo =
		preview !== null &&
		preview.changes.length === 0 &&
		preview.conflicts.length === 0;

	return (
		<div
			className="flex h-full flex-col"
			style={{ background: "var(--box-bg)", fontSize: "var(--box-font-size)" }}
		>
			<header
				className="flex shrink-0 items-center px-4"
				style={{ height: "54px", borderBottom: "1px solid var(--shadow-fg)" }}
			>
				<div className="min-w-0 flex-1">
					<h2 className="font-semibold text-[15px] text-[var(--window-bold-fg)]">
						Review changes
					</h2>
					{preview && (
						<p className="truncate text-[12px] text-[var(--window-sub-text-fg)]">
							{preview.remote}
						</p>
					)}
				</div>
				<button
					type="button"
					onClick={onClose}
					className="cursor-pointer rounded-[6px] px-2 py-1"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Close
				</button>
			</header>

			<div className="min-h-0 flex-1 overflow-y-auto p-4">
				{error && (
					<p style={{ color: "var(--attention-button-fg)" }}>{error}</p>
				)}

				{!preview && !error && (
					<p className="text-[var(--window-sub-text-fg)]">
						Reading the other copy…
					</p>
				)}

				{outcome && (
					<p
						className="mb-4 rounded-[6px] px-3 py-2"
						style={{
							background: "var(--window-bg-over)",
							color: "var(--window-fg)",
						}}
					>
						{outcome.detail}
						{outcome.racesLost > 0 &&
							` Another device overwrote us ${outcome.racesLost} time(s); the merge was redone.`}
					</p>
				)}

				{!outcome &&
					preview?.warnings.map((w) => (
						<p
							key={w.kind}
							className="mb-3 rounded-[6px] px-3 py-2"
							style={{
								background: "var(--window-bg-over)",
								color: "var(--window-fg)",
								borderLeft: "2px solid var(--attention-button-fg)",
							}}
						>
							{w.detail}
						</p>
					))}

				{preview?.firstSync && !outcome && (
					<p
						className="mb-4 rounded-[6px] px-3 py-2"
						style={{
							background: "var(--window-bg-over)",
							color: "var(--window-sub-text-fg)",
						}}
					>
						First sync for this file, so there is no common history to compare
						against. Nothing will be deleted: an entry only one side has is
						indistinguishable from one the other side deleted.
					</p>
				)}

				{nothingToDo && !outcome && (
					<p className="text-[var(--window-sub-text-fg)]">
						{preview.remoteUnchanged
							? "The other copy has not changed since the last sync."
							: "Nothing to merge."}
					</p>
				)}

				{preview && preview.conflicts.length > 0 && (
					<section className="mb-5">
						<Heading>Needs a decision · {preview.conflicts.length}</Heading>
						{preview.conflicts.map((c) => (
							<Item
								key={`${c.kind}-${c.title}`}
								title={c.title}
								detail={c.detail}
								accent="var(--attention-button-fg)"
							/>
						))}

						<fieldset
							className="mt-3 rounded-[6px] p-3"
							style={{
								background: "var(--window-bg-over)",
								border: "1px solid var(--shadow-fg)",
							}}
						>
							<legend className="px-1 text-[var(--window-sub-text-fg)]">
								For each of these
							</legend>
							<Choice
								name="Keep both"
								hint="Neither password is thrown away. The duplicate is marked so you can tidy up later."
								value="duplicate"
								current={policy}
								onChange={setPolicy}
							/>
							<Choice
								name="Keep this device's version"
								hint="The other device's version is preserved in history, not deleted."
								value="keep-local"
								current={policy}
								onChange={setPolicy}
							/>
							<Choice
								name="Keep the other device's version"
								hint="This device's version is preserved in history, not deleted."
								value="keep-remote"
								current={policy}
								onChange={setPolicy}
							/>
							<Choice
								name="Do nothing for now"
								hint="Applies everything above but leaves these alone, and uploads nothing."
								value="refuse"
								current={policy}
								onChange={setPolicy}
							/>
						</fieldset>
					</section>
				)}

				{preview && preview.changes.length > 0 && (
					<section>
						<Heading>
							Will apply automatically · {preview.changes.length}
						</Heading>
						{preview.changes.map((c) => (
							<Item
								key={`${c.kind}-${c.title}`}
								title={c.title}
								detail={c.detail}
								accent={
									c.kind.includes("deleted")
										? "var(--attention-button-fg)"
										: "var(--window-active-text-fg)"
								}
							/>
						))}
					</section>
				)}
			</div>

			<footer
				className="flex shrink-0 items-center gap-2 px-4 py-3"
				style={{ borderTop: "1px solid var(--shadow-fg)" }}
			>
				<button
					type="button"
					disabled={applying || !preview || nothingToDo}
					onClick={() => void applyMerge()}
					className="cursor-pointer rounded-[6px] px-3 py-1.5 font-semibold disabled:opacity-45"
					style={{
						background: "var(--active-button-bg)",
						color: "var(--window-fg-active)",
					}}
				>
					{applying ? "Merging…" : "Merge and upload"}
				</button>
				<button
					type="button"
					onClick={onClose}
					className="cursor-pointer rounded-[6px] px-3 py-1.5"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Cancel
				</button>
				<span className="ml-auto text-[12px] text-[var(--window-sub-text-fg)]">
					Every version stays in the file's history.
				</span>
			</footer>
		</div>
	);
}

function Heading({ children }: { children: React.ReactNode }) {
	return (
		<h3
			className="mb-2 font-semibold uppercase"
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

function Item({
	title,
	detail,
	accent,
}: {
	title: string;
	detail: string;
	accent: string;
}) {
	return (
		<div
			className="flex gap-3 py-2"
			style={{ borderBottom: "1px solid var(--shadow-fg)" }}
		>
			<span
				aria-hidden
				className="mt-1.5 h-2 w-2 shrink-0 rounded-full"
				style={{ background: accent }}
			/>
			<div className="min-w-0">
				<div className="truncate font-semibold text-[var(--window-bold-fg)]">
					{title}
				</div>
				<div className="text-[var(--window-sub-text-fg)]">{detail}</div>
			</div>
		</div>
	);
}

function Choice({
	name,
	hint,
	value,
	current,
	onChange,
}: {
	name: string;
	hint: string;
	value: ConflictPolicy;
	current: ConflictPolicy;
	onChange: (p: ConflictPolicy) => void;
}) {
	return (
		<label className="flex cursor-pointer items-start gap-2 py-1">
			<input
				type="radio"
				name="conflict-policy"
				checked={current === value}
				onChange={() => onChange(value)}
				className="mt-1"
				style={{ accentColor: "var(--window-bg-active)" }}
			/>
			<span className="min-w-0">
				<span className="block text-[var(--window-fg)]">{name}</span>
				<span className="block text-[12px] text-[var(--window-sub-text-fg)]">
					{hint}
				</span>
			</span>
		</label>
	);
}
