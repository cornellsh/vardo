import { useEffect, useRef, useState } from "react";

import { Presence } from "../components/Presence";
import { VardoMark } from "../components/VardoMark";
import { VaultPicker } from "../components/VaultPicker";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { UnlockPhase, VaultInfo, VaultSource } from "../ipc";

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
	const [source, setSource] = useState<VaultSource | null>(null);
	const [password, setPassword] = useState("");
	const [phase, setPhase] = useState<UnlockPhase | null>(null);
	const [error, setError] = useState<string | null>(null);
	/** Held after `error` clears, so the exit animation has text to play on. */
	const [lastError, setLastError] = useState("");
	/** Bumped on every failure, to re-trigger the shake. */
	const [shakeKey, setShakeKey] = useState(0);
	const passwordRef = useRef<HTMLInputElement>(null);

	const busy = phase !== null;

	useEffect(() => {
		passwordRef.current?.focus();
	}, []);

	async function submit(e: React.FormEvent) {
		e.preventDefault();
		if (busy || !password || !source) return;
		setError(null);
		setPhase({ phase: "reading", progress: null });
		try {
			const api = await ipc();
			const info = await api.openVault(source, password, setPhase);
			setPassword("");
			onUnlocked(info);
		} catch (err) {
			const kind = (err as { kind?: string })?.kind;
			// Safe to log: UiError carries a reason and never the password or any
			// field value. Without this an unrecognised kind is indistinguishable
			// from a wrong password, which makes a bridge bug look like user error.
			console.error("unlock failed", JSON.stringify(err));
			const message =
				kind === "wrong-password"
					? "Wrong password."
					: kind === "not-found"
						? source.kind === "file"
							? "That file is no longer there. If it is on a removable or network drive, it may just not be mounted."
							: "That vault is no longer in your Drive. It may have been deleted or moved to the bin."
						: // Offline and auth failures must never be shown as a wrong
							// password: the fix is in a different place entirely, and
							// sending someone to re-check a password that is correct is
							// the single most frustrating thing this screen can do.
							kind === "offline"
							? errorText(err)
							: kind === "drive-auth-expired"
								? "Vardo's access to your Google account has expired or was withdrawn. Reconnect it and try again."
								: kind === "unsupported"
									? `Vardo cannot read this database yet: ${errorText(err)}`
									: // An unrecognised failure shows its reason rather than a
										// generic sentence. "Could not open the database" on its own
										// is untriageable for the user and for us.
										`Could not open the database: ${errorText(err)}`;
			setLastError(message);
			setError(message);
			setPassword("");
			setPhase(null);
			setShakeKey((k) => k + 1);
			passwordRef.current?.focus();
		}
	}

	const progress = phase?.phase === "deriving" ? phase.progress : null;

	return (
		<div className="flex h-full items-center justify-center bg-[var(--window-bg)] px-6">
			<form onSubmit={submit} className="w-full max-w-[320px]">
				{/* The mark, standing on its own. It used to be a stock padlock
				    inside a filled circle, which is what you reach for when the glyph
				    is generic and needs something to make it look deliberate. */}
				<div className="mb-7 text-center">
					<div className="mb-4 flex justify-center">
						<VardoMark size={56} />
					</div>
					<h1 className="font-semibold text-[20px] text-[var(--window-bold-fg)]">
						Vardo
					</h1>
				</div>

				{/* Which vault, before the password. Asking for a password first
				    and the vault second would be backwards: the password only
				    means anything once you know what it is for, and someone with
				    two vaults would routinely type the wrong one. */}
				<div className="mb-4">
					<VaultPicker selected={source} onSelect={setSource} disabled={busy} />
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
						transitionDuration: "var(--duration-universal)",
						// A rejected password shakes the field, which is what every
						// lock screen worth using does: it says "try again" without
						// making you read anything. `shakeDuration: 300` from
						// lib_ui/ui/basic.style. Keyed on `shakeKey` so a second
						// wrong attempt restarts the animation rather than being
						// swallowed because the class never changed.
						animation: error
							? "shake var(--duration-shake) var(--ease-sine-in-out)"
							: undefined,
					}}
					key={shakeKey}
				/>

				{/* A fade rather than a collapse, because the row already reserves
				    its height. Animating the box would move the button below it,
				    and a button that shifts as you are told why the last attempt
				    failed is how you click the wrong thing. */}
				<div aria-live="polite" className="min-h-[20px] pt-2 text-center">
					<Presence
						show={!!error}
						enter="fade-in"
						exit="fade-out"
						enterDuration="var(--duration-universal)"
						exitDuration="var(--duration-universal)"
						exitMs={120}
					>
						<span
							id="unlock-error"
							className="text-[var(--attention-button-fg)]"
						>
							{lastError}
						</span>
					</Presence>
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
					disabled={busy || !password || !source}
					className="btn btn-accent mt-4 w-full cursor-pointer rounded-[6px] px-3 py-2.5 font-semibold"
					style={{
						color: "var(--window-fg-active)",
					}}
				>
					{busy ? "Unlocking…" : "Unlock"}
				</button>
			</form>
		</div>
	);
}
