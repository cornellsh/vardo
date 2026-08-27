import { useCallback, useEffect, useState } from "react";

import { DriveLogo } from "./DriveLogo";
import { Notice, splitTechnical } from "./Notice";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { DriveVault, VaultSource } from "../ipc";

/**
 * Choosing which vault to open.
 *
 * The organising idea is that "where is your vault" is one question with
 * several answers, not several unrelated features. So recents, a file, and
 * Drive all live in one list, and a recent Drive vault looks the same as a
 * recent local one apart from its icon. Nobody has to know what a backend is.
 *
 * Adding WebDAV later means another entry in `sources` and another icon. It
 * does not mean redesigning this screen, which is the point of doing it this
 * way rather than bolting a "Google Drive" button onto the side.
 */

export function sourceKey(s: VaultSource): string {
	return s.kind === "file" ? `file:${s.path}` : `gdrive:${s.fileId}`;
}

export function sourceName(s: VaultSource): string {
	const raw = s.kind === "file" ? (s.path.split("/").pop() ?? s.path) : s.name;
	return raw.replace(/\.kdbx$/i, "");
}

function sourceWhere(s: VaultSource): string {
	if (s.kind === "googleDrive") return "Google Drive";
	const parts = s.path.split("/");
	parts.pop();
	return parts.join("/") || "/";
}

interface Props {
	selected: VaultSource | null;
	onSelect: (source: VaultSource) => void;
	disabled: boolean;
}

export function VaultPicker({ selected, onSelect, disabled }: Props) {
	const [recents, setRecents] = useState<VaultSource[]>([]);
	const [driveOpen, setDriveOpen] = useState(false);

	useEffect(() => {
		void (async () => {
			const list = await (await ipc()).recentVaults();
			setRecents(list);
			// Pre-select the most recent, because the overwhelmingly common case
			// is reopening the same vault and it should take one keystroke.
			if (list[0] && !selected) onSelect(list[0]);
		})();
		// Intentionally once: re-running on `selected` would fight the user.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, []);

	async function browse() {
		const picked = await (await ipc()).pickVaultFile();
		if (picked) onSelect({ kind: "file", path: picked });
	}

	async function forget(source: VaultSource) {
		await (await ipc()).forgetVault(source);
		setRecents((r) => r.filter((s) => sourceKey(s) !== sourceKey(source)));
	}

	return (
		<div className="flex flex-col gap-1">
			{recents.map((source) => {
				const isSelected =
					selected && sourceKey(selected) === sourceKey(source);
				return (
					<div key={sourceKey(source)} className="group relative flex">
						<button
							type="button"
							disabled={disabled}
							onClick={() => onSelect(source)}
							className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-[6px] px-3 py-2 text-left transition-colors disabled:cursor-default"
							style={{
								background: isSelected
									? "var(--window-bg-active)"
									: "transparent",
								color: isSelected
									? "var(--window-fg-active)"
									: "var(--window-fg)",
								transitionDuration: "var(--duration-universal)",
							}}
						>
							<SourceIcon source={source} active={!!isSelected} />
							<span className="min-w-0 flex-1">
								<span className="block truncate font-semibold">
									{sourceName(source)}
								</span>
								<span
									className="block truncate"
									style={{
										color: isSelected
											? "var(--window-fg-active)"
											: "var(--window-sub-text-fg)",
										opacity: isSelected ? 0.75 : 1,
									}}
								>
									{sourceWhere(source)}
								</span>
							</span>
						</button>

						{/* A recents entry can outlive the thing it points at - an
						    unplugged drive, a file deleted elsewhere. An entry that
						    can only ever fail has to be removable. */}
						<button
							type="button"
							aria-label={`Remove ${sourceName(source)} from this list`}
							title="Remove from this list"
							onClick={() => void forget(source)}
							className="absolute top-0 right-1 bottom-0 my-auto h-7 w-7 cursor-pointer rounded-[var(--button-radius)] opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
							style={{
								color: isSelected
									? "var(--window-fg-active)"
									: "var(--window-sub-text-fg)",
								transitionDuration: "var(--duration-universal)",
							}}
						>
							&times;
						</button>
					</div>
				);
			})}

			<div className="mt-1 flex gap-2">
				<button
					type="button"
					disabled={disabled}
					onClick={() => void browse()}
					className="flex flex-1 cursor-pointer items-center justify-center gap-2 rounded-[6px] py-2 font-semibold transition-colors"
					style={{
						background: "var(--window-bg-over)",
						color: "var(--window-active-text-fg)",
						transitionDuration: "var(--duration-universal)",
					}}
				>
					Open a file&hellip;
				</button>
				<button
					type="button"
					disabled={disabled}
					onClick={() => setDriveOpen((o) => !o)}
					aria-expanded={driveOpen}
					className="flex flex-1 cursor-pointer items-center justify-center gap-2 rounded-[6px] py-2 font-semibold transition-colors"
					style={{
						background: "var(--window-bg-over)",
						color: "var(--window-active-text-fg)",
						transitionDuration: "var(--duration-universal)",
					}}
				>
					<DriveLogo size={16} />
					Google Drive
				</button>
			</div>

			{driveOpen && <DriveList onPick={onSelect} />}
		</div>
	);
}

function SourceIcon({
	source,
	active,
}: {
	source: VaultSource;
	active: boolean;
}) {
	if (source.kind === "googleDrive") return <DriveLogo size={20} />;
	return (
		<svg
			width="20"
			height="20"
			viewBox="0 0 24 24"
			fill="none"
			stroke={active ? "var(--window-fg-active)" : "var(--window-sub-text-fg)"}
			strokeWidth="1.8"
			aria-hidden
		>
			<path d="M4 6a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z" />
		</svg>
	);
}

/**
 * The Drive side of the chooser.
 *
 * The empty state carries most of the weight here, because under the
 * `drive.file` scope it is the *expected* state for someone who already keeps a
 * vault in Drive: Vardo genuinely cannot see it until it is handed over. An
 * empty box with no explanation would read as a broken feature, and the natural
 * next thought would be that Vardo does not really work.
 */
function DriveList({ onPick }: { onPick: (s: VaultSource) => void }) {
	const [state, setState] = useState<
		| { phase: "loading" }
		| { phase: "disconnected" }
		| { phase: "ready"; vaults: DriveVault[] }
		| { phase: "error"; message: string }
	>({ phase: "loading" });

	const load = useCallback(async () => {
		setState({ phase: "loading" });
		try {
			const api = await ipc();
			const status = await api.driveStatus();
			if (!status.connected) {
				setState({ phase: "disconnected" });
				return;
			}
			setState({ phase: "ready", vaults: await api.driveListVaults() });
		} catch (err) {
			setState({ phase: "error", message: errorText(err) });
		}
	}, []);

	useEffect(() => {
		void load();
	}, [load]);

	async function connect() {
		setState({ phase: "loading" });
		try {
			await (await ipc()).driveConnect();
			await load();
		} catch (err) {
			setState({ phase: "error", message: errorText(err) });
		}
	}

	async function uploadExisting() {
		const picked = await (await ipc()).pickVaultFile();
		if (!picked) return;
		setState({ phase: "loading" });
		try {
			onPick(await (await ipc()).driveUploadVault(picked));
			await load();
		} catch (err) {
			setState({ phase: "error", message: errorText(err) });
		}
	}

	return (
		<div
			className="mt-1 rounded-[6px] p-3"
			style={{
				background: "var(--window-bg-over)",
				animation: "slide-up var(--duration-slide-wrap) var(--ease-out-circ)",
			}}
		>
			{state.phase === "loading" && (
				<p style={{ color: "var(--window-sub-text-fg)" }}>Looking&hellip;</p>
			)}

			{state.phase === "disconnected" && (
				<>
					<p style={{ color: "var(--window-sub-text-fg)" }}>
						Sign in to open a vault kept in Drive.
					</p>
					<button
						type="button"
						onClick={() => void connect()}
						className="mt-2 flex cursor-pointer items-center gap-2 rounded-[var(--button-radius)] px-3 py-1.5 font-semibold"
						style={{
							background: "var(--active-button-bg)",
							color: "var(--window-fg-active)",
						}}
					>
						<DriveLogo size={16} />
						Sign in
					</button>
				</>
			)}

			{state.phase === "error" &&
				(() => {
					const { message, technical } = splitTechnical(state.message);
					return (
						<Notice
							title={message}
							{...(technical ? { technical } : {})}
							action={{ label: "Try again", onClick: () => void load() }}
						/>
					);
				})()}

			{state.phase === "ready" && state.vaults.length === 0 && (
				<>
					<p style={{ color: "var(--window-sub-text-fg)" }}>
						Nothing here yet. Vardo only sees files you give it.
					</p>
					<button
						type="button"
						onClick={() => void uploadExisting()}
						className="mt-2 cursor-pointer font-semibold"
						style={{ color: "var(--window-active-text-fg)" }}
					>
						Add a vault from this computer&hellip;
					</button>
				</>
			)}

			{state.phase === "ready" &&
				state.vaults.map((v) => (
					<button
						key={v.fileId}
						type="button"
						onClick={() =>
							onPick({ kind: "googleDrive", fileId: v.fileId, name: v.name })
						}
						className="flex w-full cursor-pointer items-center gap-3 rounded-[6px] px-2 py-2 text-left transition-colors hover:bg-[var(--window-bg-ripple)]"
						style={{ transitionDuration: "var(--duration-universal)" }}
					>
						<DriveLogo size={18} />
						<span className="min-w-0 flex-1 truncate">
							{v.name.replace(/\.kdbx$/i, "")}
						</span>
						<span style={{ color: "var(--window-sub-text-fg)" }}>
							{formatSize(v.size)}
						</span>
					</button>
				))}
		</div>
	);
}

function formatSize(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
