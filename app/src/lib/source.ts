import type { VaultSource } from "../ipc/types";

/** One key per vault, matching Rust's `VaultSource::stable_key` in spirit. */
export function sourceKey(s: VaultSource): string {
	switch (s.kind) {
		case "file":
			return `file:${s.path}`;
		case "googleDrive":
			return `gdrive:${s.file_id}`;
		case "webDav":
			return `webdav:${s.username}@${s.url}`;
	}
}

export function sourceName(s: VaultSource): string {
	const raw =
		s.kind === "file"
			? (s.path.split("/").pop() ?? s.path)
			: s.kind === "webDav"
				? (s.url.split("/").pop() ?? s.url)
				: s.name;
	return raw.replace(/\.kdbx$/i, "");
}

/** The host of a WebDAV address, for the line under a vault's name. */
export function webHost(url: string): string {
	return url.replace(/^[a-z]+:\/\//i, "").split(/[/?#]/)[0] ?? url;
}
