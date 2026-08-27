//! Finding and creating vaults in Drive.
//!
//! The constraint that shapes this module: under the `drive.file` scope, Drive
//! only tells us about files this app created or that the user explicitly
//! handed over. A vault that already sits in someone's Drive is invisible to
//! [`list`] until then.
//!
//! That is the scope doing its job, not a bug to route around. The alternative
//! is asking for read access to the user's entire Drive purely to populate a
//! list, which for a password manager is a grotesque thing to request and
//! exactly the sort of over-reach that makes people distrust these programs.
//!
//! So the answer to an empty list is [`upload`], not a wider scope: put the
//! vault there once, and from then on it is app-created and permanently
//! visible. The UI has to say this plainly, because an empty list with no
//! explanation reads as "Vardo is broken".

use std::path::Path;

use vardo_drive::drive::DriveFile;

use crate::drive_auth::{self, DriveClient};
use crate::session::CoreError;
use crate::source::VaultSource;

/// Vaults in Drive that this app is allowed to see, newest first.
pub fn list(client: &DriveClient, app_dir: &Path) -> Result<Vec<DriveFile>, CoreError> {
    let tokens = drive_auth::access(client, app_dir)?;
    let mut http = vardo_drive::ureq_transport::UreqTransport::new();

    let (files, updated_tokens, changed) =
        vardo_drive::drive::list_vaults(&mut http, client.config(), tokens)
            .map_err(|e| crate::sync::map_sync_error(e, "Google Drive"))?;

    if changed {
        let _ = drive_auth::store_tokens(app_dir, &updated_tokens);
    }
    Ok(files)
}

/// Copies a local vault into Drive and returns it as a source.
///
/// The local file is left exactly where it is. Moving it would mean deleting
/// someone's only copy of their passwords as a side effect of a button labelled
/// "upload", and if the upload half-succeeded there would be nothing to go
/// back to.
pub fn upload(
    client: &DriveClient,
    app_dir: &Path,
    local: &Path,
) -> Result<VaultSource, CoreError> {
    let bytes = std::fs::read(local).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => CoreError::NotFound {
            path: local.display().to_string(),
        },
        _ => CoreError::Io {
            detail: e.to_string(),
        },
    })?;

    // Refuse to upload something that is not a vault. Without this, a mistyped
    // path would put an arbitrary file in Drive under a .kdbx name and then
    // fail confusingly at unlock instead of here, where the cause is obvious.
    if !looks_like_kdbx(&bytes) {
        return Err(CoreError::Corrupt {
            detail: format!(
                "{} does not look like a KDBX file, so it was not uploaded.",
                local.display()
            ),
        });
    }

    let name = local
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vault.kdbx".into());

    let tokens = drive_auth::access(client, app_dir)?;
    let mut http = vardo_drive::ureq_transport::UreqTransport::new();

    let (file_id, updated_tokens, changed) =
        vardo_drive::drive::create(&mut http, client.config(), tokens, &name, &bytes)
            .map_err(|e| crate::sync::map_sync_error(e, "Google Drive"))?;

    if changed {
        let _ = drive_auth::store_tokens(app_dir, &updated_tokens);
    }

    Ok(VaultSource::GoogleDrive { file_id, name })
}

/// The KDBX magic. Cheap, and catches the whole class of "wrong file".
fn looks_like_kdbx(bytes: &[u8]) -> bool {
    bytes.len() > 8 && bytes[0..4] == [0x03, 0xD9, 0xA2, 0x9A]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_a_real_vault_header() {
        let mut bytes = vec![0x03, 0xD9, 0xA2, 0x9A, 0x67, 0xFB, 0x4B, 0xB5, 0x00];
        assert!(looks_like_kdbx(&bytes));
        bytes[0] = 0x00;
        assert!(!looks_like_kdbx(&bytes));
    }

    #[test]
    fn rejects_a_file_too_short_to_have_a_header() {
        // Guards the slice: an empty or truncated file must be refused rather
        // than panic on the index.
        assert!(!looks_like_kdbx(&[]));
        assert!(!looks_like_kdbx(&[0x03, 0xD9]));
    }
}
