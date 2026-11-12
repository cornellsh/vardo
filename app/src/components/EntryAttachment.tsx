import { useState } from "react";

import { ipc } from "../ipc";
import type { Uuid } from "../ipc";

export function AttachmentRow({
	uuid,
	name,
	size,
	onRemoved,
}: {
	uuid: Uuid;
	name: string;
	size: number | null;
	/** Absent on a read-only vault, where there is nothing to detach. */
	onRemoved?: (() => void) | undefined;
}) {
	const [state, setState] = useState<"idle" | "saving" | "done" | "failed">(
		"idle",
	);

	async function detach() {
		// Asked, because the bytes are only in the vault. Unlike an entry there
		// is no history to recover an attachment from, so this is the one
		// deletion here that is genuinely final.
		if (!window.confirm(`Remove ${name} from this entry?`)) return;
		try {
			await (await ipc()).removeAttachment(uuid, name);
			onRemoved?.();
		} catch {
			setState("failed");
		}
	}

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
			{onRemoved && (
				<button
					type="button"
					onClick={() => void detach()}
					aria-label={`Remove ${name}`}
					className="on-hover shrink-0 cursor-pointer rounded-[6px] px-2 py-0.5"
					style={{ color: "var(--attention-button-fg)" }}
				>
					Remove
				</button>
			)}
		</div>
	);
}

export function formatBytes(bytes: number): string {
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
