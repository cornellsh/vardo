import { useState } from "react";

import { Notice, splitTechnical } from "./Notice";
import { errorText } from "../ipc/error";
import { ipc } from "../ipc";
import type { VaultSource } from "../ipc";

interface Props {
	onConnected: (source: VaultSource) => void;
}

/**
 * Adding a vault that lives on a WebDAV server, such as Nextcloud.
 *
 * Connecting reads the file once, so a wrong address or password fails here,
 * next to the boxes that caused it, rather than after the master password.
 * The server password goes to the system keychain, not into Vardo's lists.
 */
export function WebDavForm({ onConnected }: Props) {
	const [url, setUrl] = useState("");
	const [username, setUsername] = useState("");
	const [password, setPassword] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState<string | null>(null);

	async function connect(e: React.FormEvent) {
		e.preventDefault();
		setBusy(true);
		setError(null);
		try {
			const source = await (await ipc()).webdavConnect(url, username, password);
			setPassword("");
			onConnected(source);
		} catch (err) {
			setError(errorText(err));
		} finally {
			setBusy(false);
		}
	}

	const field = "w-full rounded-[4px] px-2 py-1.5 outline-none";
	const fieldStyle = { background: "var(--window-bg)", border: "1px solid var(--shadow-fg)" };

	return (
		<form
			onSubmit={(e) => void connect(e)}
			className="mt-2 flex flex-col gap-2 rounded-[6px] p-3"
			style={{ background: "var(--window-bg-over)" }}
		>
			<label className="flex flex-col gap-1">
				<span className="text-[var(--window-sub-text-fg)]">Address of the vault file</span>
				<input
					type="url"
					required
					value={url}
					onChange={(e) => setUrl(e.target.value)}
					placeholder="https://cloud.example.com/remote.php/dav/files/you/passwords.kdbx"
					spellCheck={false}
					className={field}
					style={fieldStyle}
				/>
			</label>
			<label className="flex flex-col gap-1">
				<span className="text-[var(--window-sub-text-fg)]">Username</span>
				<input
					required
					value={username}
					onChange={(e) => setUsername(e.target.value)}
					autoComplete="username"
					spellCheck={false}
					className={field}
					style={fieldStyle}
				/>
			</label>
			<label className="flex flex-col gap-1">
				<span className="text-[var(--window-sub-text-fg)]">Server password</span>
				<input
					type="password"
					required
					value={password}
					onChange={(e) => setPassword(e.target.value)}
					autoComplete="current-password"
					className={field}
					style={fieldStyle}
				/>
			</label>
			<button
				type="submit"
				disabled={busy}
				className="btn cursor-pointer rounded-[var(--button-radius)] py-1.5 font-semibold transition-colors disabled:opacity-45"
				style={{
					background: "var(--active-button-bg)",
					color: "var(--window-fg-active)",
					transitionDuration: "var(--duration-universal)",
				}}
			>
				{busy ? "Connecting…" : "Connect"}
			</button>
			{error &&
				(() => {
					const { message, technical } = splitTechnical(error);
					return <Notice title={message} {...(technical ? { technical } : {})} />;
				})()}
		</form>
	);
}
