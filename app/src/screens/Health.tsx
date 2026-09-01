import { useEffect, useState } from "react";

import { Presence } from "../components/Presence";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { Finding, HealthReport, Issue, Uuid } from "../ipc";

/**
 * What is wrong with the passwords in this vault.
 *
 * The one thing a password manager does that a text file cannot: look at the
 * whole set at once. Storing secrets is the easy half, and a vault that only
 * stores them is a filing cabinet.
 *
 * Grouped by problem rather than listed flat, because the answer to each is
 * different. Reuse means changing one of them; weak means generating a new one;
 * expired means the account itself has a deadline. A single list sorted by
 * severity would make you re-read the category on every row.
 */

const ISSUES: {
	kind: Issue;
	title: string;
	/** What it means, in one line, for someone who has not thought about it. */
	why: string;
	tone: string;
}[] = [
	{
		kind: "reused",
		title: "Used more than once",
		why: "One breach opens every account sharing the password.",
		tone: "var(--attention-button-fg)",
	},
	{
		kind: "weak",
		title: "Easy to guess",
		why: "Short, or built from one kind of character.",
		tone: "#e8a33d",
	},
	{
		kind: "expired",
		title: "Past its expiry date",
		why: "You set a date on these and it has passed.",
		tone: "#e8a33d",
	},
	{
		kind: "stale",
		title: "Not changed in years",
		why: "Worth rotating if the site has been breached since.",
		tone: "var(--window-sub-text-fg)",
	},
];

interface Props {
	onClose: () => void;
	onOpenEntry: (uuid: Uuid) => void;
}

export function Health({ onClose, onOpenEntry }: Props) {
	const [report, setReport] = useState<HealthReport | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			try {
				const r = await (await ipc()).health();
				if (!cancelled) setReport(r);
			} catch (err) {
				if (!cancelled) setError(errorText(err));
			}
		})();
		return () => {
			cancelled = true;
		};
	}, []);

	const groups = ISSUES.map((issue) => ({
		...issue,
		found: report?.findings.filter((f) => f.issue === issue.kind) ?? [],
	})).filter((g) => g.found.length > 0);

	return (
		<section
			aria-label="Password health"
			className="flex min-w-0 flex-1 flex-col"
			style={{
				minWidth: "var(--column-min-third)",
				background: "var(--window-bg)",
				animation: "fade-in var(--duration-fade-wrap) var(--ease-out-circ)",
			}}
		>
			<header
				className="flex shrink-0 items-center px-4"
				style={{ height: "54px", borderBottom: "1px solid var(--shadow-fg)" }}
			>
				<div className="min-w-0 flex-1">
					<h2 className="font-semibold text-[15px] text-[var(--window-bold-fg)]">
						Password health
					</h2>
					{report && (
						<p className="text-[12px] text-[var(--window-sub-text-fg)]">
							{/* Stated against a total. A bare "31 problems" could mean
							    anything until you know whether the vault holds forty
							    entries or four hundred. */}
							{report.checked} password
							{report.checked === 1 ? "" : "s"} checked
						</p>
					)}
				</div>
				<button
					type="button"
					onClick={onClose}
					className="btn btn-quiet cursor-pointer rounded-[var(--button-radius)] px-2 py-1"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					Close
				</button>
			</header>

			<div className="min-h-0 flex-1 overflow-y-auto p-4">
				<Presence show={!!error}>
					<p style={{ color: "var(--attention-button-fg)" }}>{error}</p>
				</Presence>

				{!report && !error && (
					<p style={{ color: "var(--window-sub-text-fg)" }}>Checking…</p>
				)}

				{report && groups.length === 0 && (
					// Worth saying properly. "Nothing found" reads as the check having
					// failed to run; this is the one screen where a good result is the
					// whole point and deserves to look like one.
					<div className="pt-8 text-center">
						<p className="font-semibold text-[15px] text-[var(--window-bold-fg)]">
							Nothing to fix
						</p>
						<p className="mt-1 text-[var(--window-sub-text-fg)]">
							No reused, weak or expired passwords.
						</p>
					</div>
				)}

				{groups.map((group) => (
					<section key={group.kind} className="mb-6">
						<div className="mb-1 flex items-baseline gap-2">
							<h3
								className="font-semibold text-[13px]"
								style={{ color: group.tone }}
							>
								{group.title}
							</h3>
							<span
								className="tabular-nums text-[12px]"
								style={{ color: "var(--window-sub-text-fg)" }}
							>
								{group.found.length}
							</span>
						</div>
						<p className="mb-2 text-[12px] text-[var(--window-sub-text-fg)]">
							{group.why}
						</p>

						{group.found.map((finding) => (
							<FindingRow
								key={`${finding.issue}-${finding.uuid}`}
								finding={finding}
								onOpen={() => onOpenEntry(finding.uuid)}
							/>
						))}
					</section>
				))}
			</div>
		</section>
	);
}

function FindingRow({
	finding,
	onOpen,
}: {
	finding: Finding;
	onOpen: () => void;
}) {
	return (
		<button
			type="button"
			onClick={onOpen}
			className="btn btn-quiet -mx-2 flex w-[calc(100%+1rem)] cursor-pointer items-center gap-2 rounded-[6px] px-2 py-1.5 text-left"
		>
			<span className="min-w-0 flex-1 truncate">
				{finding.title || "Untitled"}
			</span>
			{/* The count, never the other entries. Naming them would turn one
			    finding into a map of which accounts fall together. */}
			{finding.sharedWith !== null && (
				<span
					className="shrink-0 tabular-nums text-[12px]"
					style={{ color: "var(--window-sub-text-fg)" }}
				>
					{finding.sharedWith} entries
				</span>
			)}
		</button>
	);
}
