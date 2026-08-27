import { useEffect, useRef, useState } from "react";

import { ipc } from "../ipc";
import type { UnlockPhase, VaultInfo } from "../ipc";

interface Props {
	onUnlocked: (info: VaultInfo) => void;
	lockedReason?: "idle" | "manual" | undefined;
}

const PHASE_LABEL: Record<UnlockPhase["phase"], string> = {
	reading: "Reading database",
	deriving: "Deriving key",
	decrypting: "Decrypting",
	parsing: "Reading entries",
};

/**
 * Argon2d on the reference vault measures 0.871s, and closer to 3s on older
 * hardware. A synchronous unlock with no feedback is exactly what makes
 * KeePassXC feel broken, so progress is not decoration here — it is the
 * difference between "working" and "hung".
 */
export function Unlock({ onUnlocked, lockedReason }: Props) {
	const [path, setPath] = useState("");
	const [password, setPassword] = useState("");
	const [phase, setPhase] = useState<UnlockPhase | null>(null);
	const [error, setError] = useState<string | null>(null);
	const passwordRef = useRef<HTMLInputElement>(null);

	const busy = phase !== null;

	useEffect(() => {
		let cancelled = false;
		void (async () => {
			const recent = await (await ipc()).recentVaults();
			if (!cancelled && recent[0]) setPath(recent[0]);
		})();
		return () => {
			cancelled = true;
		};
	}, []);

	useEffect(() => {
		passwordRef.current?.focus();
	}, []);

	async function submit(e: React.FormEvent) {
		e.preventDefault();
		if (busy || !password) return;
		setError(null);
		setPhase({ phase: "reading" });
		try {
			const api = await ipc();
			const info = await api.openVault(path, password, setPhase);
			setPassword("");
			onUnlocked(info);
		} catch (err) {
			const kind = (err as { kind?: string })?.kind;
			// Safe to log: UiError carries a reason and never the password or any
			// field value. Without this an unrecognised kind is indistinguishable
			// from a wrong password, which makes a bridge bug look like user error.
			console.error("unlock failed", JSON.stringify(err));
			const detail = (err as { detail?: string })?.detail;
			setError(
				kind === "wrong-password"
					? "Wrong password."
					: kind === "not-found"
						? "That file no longer exists."
						: kind === "unsupported"
							? `Vardo cannot read this database yet: ${detail ?? "unknown feature"}`
							: // An unrecognised failure shows its reason rather than a
								// generic sentence. "Could not open the database" on its own
								// is untriageable for the user and for us.
								`Could not open the database: ${detail ?? kind ?? String(err)}`,
			);
			setPassword("");
			setPhase(null);
			passwordRef.current?.focus();
		}
	}

	async function browse() {
		const picked = await (await ipc()).pickVaultFile();
		if (picked) setPath(picked);
	}

	const fileName = path.split("/").pop() ?? "";
	const progress = phase?.phase === "deriving" ? phase.progress : null;

	return (
		<div className="flex h-full items-center justify-center bg-[var(--window-bg)] px-6">
			<form onSubmit={submit} className="w-full max-w-[320px]">
				<div className="mb-7 text-center">
					<div
						aria-hidden
						className="mx-auto mb-4 flex h-[64px] w-[64px] items-center justify-center rounded-full"
						style={{ background: "var(--accent-behind-text)" }}
					>
						<svg
							width="28"
							height="28"
							viewBox="0 0 24 24"
							fill="none"
							stroke="var(--window-fg-active)"
							strokeWidth="2"
							strokeLinecap="round"
						>
							<rect x="4" y="10" width="16" height="10" rx="2" />
							<path d="M8 10V7a4 4 0 0 1 8 0v3" />
						</svg>
					</div>
					<h1 className="text-[20px] font-semibold text-[var(--window-bold-fg)]">
						Vardo
					</h1>
					{fileName && (
						<p
							className="mt-1 truncate text-[var(--window-sub-text-fg)]"
							title={path}
						>
							{fileName}
						</p>
					)}
				</div>

				{lockedReason === "idle" && !error && (
					<p className="mb-3 text-center text-[var(--window-sub-text-fg)]">
						Locked after inactivity.
					</p>
				)}

				<label htmlFor="password" className="sr-only">
					Master password
				</label>
				<input
					ref={passwordRef}
					id="password"
					type="password"
					value={password}
					disabled={busy}
					autoComplete="off"
					spellCheck={false}
					placeholder="Master password"
					onChange={(e) => setPassword(e.target.value)}
					aria-invalid={error !== null}
					aria-describedby={error ? "unlock-error" : undefined}
					className="secret w-full rounded-[6px] border px-3 py-2.5 outline-none transition-colors"
					style={{
						background: "var(--filter-input-active-bg)",
						borderColor: error
							? "var(--attention-button-fg)"
							: "color-mix(in srgb, var(--window-sub-text-fg) 35%, transparent)",
						color: "var(--window-fg)",
					}}
				/>

				<div aria-live="polite" className="min-h-[20px] pt-2 text-center">
					{error && (
						<span
							id="unlock-error"
							className="text-[var(--attention-button-fg)]"
						>
							{error}
						</span>
					)}
					{phase && (
						<span className="text-[var(--window-sub-text-fg)]">
							{PHASE_LABEL[phase.phase]}…
						</span>
					)}
				</div>

				{/* Determinate where Argon2 can report progress, indeterminate otherwise
            — never a spinner that implies less than it knows. */}
				<div
					className="mt-1 h-[2px] w-full overflow-hidden rounded-full"
					style={{ background: busy ? "var(--window-bg-over)" : "transparent" }}
				>
					{busy && (
						<div
							className="h-full transition-[width] duration-100 ease-linear"
							style={{
								background: "var(--window-bg-active)",
								width:
									progress === null ? "100%" : `${Math.round(progress * 100)}%`,
								opacity: progress === null ? 0.5 : 1,
							}}
						/>
					)}
				</div>

				<button
					type="submit"
					disabled={busy || !password}
					className="mt-4 w-full cursor-pointer rounded-[6px] px-3 py-2.5 font-semibold transition-colors disabled:cursor-default disabled:opacity-45"
					style={{
						background: "var(--active-button-bg)",
						color: "var(--window-fg-active)",
					}}
				>
					{busy ? "Unlocking…" : "Unlock"}
				</button>

				<button
					type="button"
					onClick={browse}
					disabled={busy}
					className="mt-2 w-full cursor-pointer rounded-[6px] px-3 py-2 transition-colors"
					style={{
						color: "var(--window-active-text-fg)",
						background: "transparent",
					}}
				>
					Open another database…
				</button>
			</form>
		</div>
	);
}
