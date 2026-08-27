/**
 * The real IPC implementation, talking to the Rust core over Tauri commands.
 *
 * Every method here is a thin, untyped-at-the-boundary call into Rust. The
 * types in `./types` are the contract; Rust is the enforcer. Nothing in this
 * file may add a code path that returns a secret — see the module doc on
 * `./types` for why.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

import type {
	EntryDetail,
	EntrySummary,
	FieldEdit,
	GeneratorOptions,
	ConflictPolicy,
	DriveStatus,
	DriveVault,
	GroupNode,
	VaultSource,
	Ipc,
	SaveResult,
	SyncOutcome,
	SyncPreview,
	TotpCode,
	UnlockPhase,
	Uuid,
	VaultInfo,
} from "./types";

export class TauriIpc implements Ipc {
	async openVault(
		source: VaultSource,
		password: string,
		onProgress?: (p: UnlockPhase) => void,
	): Promise<VaultInfo> {
		// Argon2 runs on a Rust worker thread and emits progress, because a ~0.9s
		// synchronous derivation on the main thread would freeze the window and
		// look exactly like the hang KeePassXC is criticised for.
		const unlisten = onProgress
			? await listen<UnlockPhase>("vault://unlock-progress", (e) =>
					onProgress(e.payload),
				)
			: undefined;
		try {
			return await invoke<VaultInfo>("open_vault", { source, password });
		} finally {
			unlisten?.();
		}
	}

	lock(): Promise<void> {
		return invoke("lock_vault");
	}

	vaultInfo(): Promise<VaultInfo> {
		return invoke("vault_info");
	}

	groups(): Promise<GroupNode> {
		return invoke("groups");
	}

	listEntriesSorted(
		group: string | null,
		query: string | null,
		sort: "name" | "recent",
	): Promise<EntrySummary[]> {
		return invoke("list_entries_sorted", { group, query, sort });
	}

	listEntries(opts: { group?: Uuid; query?: string }): Promise<EntrySummary[]> {
		return invoke("list_entries", {
			group: opts.group ?? null,
			query: opts.query ?? null,
		});
	}

	getEntry(uuid: Uuid): Promise<EntryDetail> {
		return invoke("get_entry", { uuid });
	}

	copyField(uuid: Uuid, key: string): Promise<{ clearAfterSeconds: number }> {
		// Rust writes to the clipboard itself and schedules the clear. The secret
		// never becomes a JS string.
		return invoke("copy_field", { uuid, key });
	}

	revealField(uuid: Uuid, key: string): Promise<string> {
		return invoke("reveal_field", { uuid, key });
	}

	totp(uuid: Uuid): Promise<TotpCode | null> {
		return invoke("totp", { uuid });
	}

	async pickVaultFile(): Promise<string | null> {
		const picked = await open({
			multiple: false,
			directory: false,
			filters: [{ name: "KeePass database", extensions: ["kdbx"] }],
		});
		return typeof picked === "string" ? picked : null;
	}

	saveAttachment(uuid: Uuid, name: string, dest: string): Promise<number> {
		return invoke("save_attachment", { uuid, name, dest });
	}

	async pickSavePath(suggested: string): Promise<string | null> {
		const picked = await save({ defaultPath: suggested });
		return typeof picked === "string" ? picked : null;
	}

	recentVaults(): Promise<VaultSource[]> {
		return invoke("recent_vaults");
	}

	// --- writing ---

	updateEntry(uuid: Uuid, fields: FieldEdit[]): Promise<boolean> {
		return invoke("update_entry", { uuid, fields });
	}

	createEntry(group: Uuid): Promise<Uuid> {
		return invoke("create_entry", { group });
	}

	deleteEntry(uuid: Uuid): Promise<"recycled" | "purged"> {
		return invoke("delete_entry", { uuid });
	}

	moveEntry(uuid: Uuid, group: Uuid): Promise<void> {
		return invoke("move_entry", { uuid, group });
	}

	saveVault(): Promise<SaveResult> {
		return invoke("save_vault");
	}

	isDirty(): Promise<boolean> {
		return invoke("is_dirty");
	}

	generatePassword(length: number, options: GeneratorOptions): Promise<string> {
		return invoke("generate_password", { length, options });
	}

	// --- sync ---

	syncPreview(): Promise<SyncPreview> {
		return invoke("sync_preview");
	}

	syncApply(policy: ConflictPolicy): Promise<SyncOutcome> {
		return invoke("sync_apply", { policy });
	}

	// --- Google Drive ---

	driveStatus(): Promise<DriveStatus> {
		return invoke("drive_status");
	}

	driveConnect(): Promise<void> {
		return invoke("drive_connect");
	}

	driveDisconnect(): Promise<void> {
		return invoke("drive_disconnect");
	}

	driveListVaults(): Promise<DriveVault[]> {
		return invoke("drive_list_vaults");
	}

	driveUploadVault(path: string): Promise<VaultSource> {
		return invoke("drive_upload_vault", { path });
	}

	drivePickVault(): Promise<VaultSource | null> {
		return invoke("drive_pick_vault");
	}

	fetchIcon(uuid: Uuid): Promise<string | null> {
		return invoke("fetch_icon", { uuid });
	}

	entriesMissingIcons(): Promise<Uuid[]> {
		return invoke("entries_missing_icons");
	}

	forgetVault(source: VaultSource): Promise<void> {
		return invoke("forget_vault", { source });
	}
}
