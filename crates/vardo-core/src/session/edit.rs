//! Changing a vault: entries, groups, tags, expiry and attachments in.
//!
//! Contract: every successful change marks the session dirty and nothing here
//! writes to storage. Only `Session::save` does.

use std::path::Path;

use super::{entries, Session};
use crate::model::FieldEdit;
use crate::CoreError;

impl Session {
    pub fn update_entry(&mut self, uuid: &str, fields: &[FieldEdit]) -> Result<bool, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let changed = self.vault.update_entry(id, |e| {
            for f in fields {
                match &f.value {
                    Some(value) => e.set_field(&f.key, value, f.protected),
                    None => e.remove_field(&f.key),
                }
            }
        })?;
        self.dirty |= changed;
        Ok(changed)
    }

    pub fn create_entry(&mut self, group: &str) -> Result<String, CoreError> {
        let id = entries::parse_uuid(group).ok_or(CoreError::NotFound {
            path: group.to_string(),
        })?;
        let uuid = self.vault.create_entry(id)?;
        self.dirty = true;
        self.info.entry_count = self.vault.entries().len();
        Ok(entries::uuid_text(uuid))
    }

    pub fn delete_entry(&mut self, uuid: &str) -> Result<&'static str, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let outcome = self.vault.delete_entry(id)?;
        self.dirty = true;
        self.info.entry_count = self.vault.entries().len();
        Ok(match outcome {
            vardo_vault::Deletion::Recycled => "recycled",
            vardo_vault::Deletion::Purged => "purged",
        })
    }

    pub fn move_entry(&mut self, uuid: &str, group: &str) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let to = entries::parse_uuid(group).ok_or(CoreError::NotFound {
            path: group.to_string(),
        })?;
        self.vault.move_entry(id, to)?;
        self.dirty = true;
        Ok(())
    }
}

impl Session {
    pub fn create_group(&mut self, parent: &str, name: &str) -> Result<String, CoreError> {
        let id = entries::parse_uuid(parent).ok_or(CoreError::NotFound {
            path: parent.to_string(),
        })?;
        let made = self.vault.create_group(id, name)?;
        self.dirty = true;
        Ok(entries::uuid_text(made))
    }

    /// Moves a group, with everything in it, under `parent`.
    pub fn move_group(&mut self, uuid: &str, parent: &str) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let to = entries::parse_uuid(parent).ok_or(CoreError::NotFound {
            path: parent.to_string(),
        })?;
        self.vault.move_group(id, to)?;
        self.dirty = true;
        Ok(())
    }

    pub fn rename_group(&mut self, uuid: &str, name: &str) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        self.vault.rename_group(id, name)?;
        self.dirty = true;
        Ok(())
    }

    /// Removes a group and everything in it.
    ///
    /// Reports how many entries went with it, so the interface can say so
    /// before asking rather than after. "Delete this group?" and "delete this
    /// group and the 38 passwords inside it?" are different questions.
    pub fn delete_group(&mut self, uuid: &str) -> Result<usize, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let before = self.vault.entries().len();
        self.vault.delete_group(id)?;
        let after = self.vault.entries().len();
        self.dirty = true;
        self.info.entry_count = after;
        Ok(before.saturating_sub(after))
    }

    /// How many entries a group holds, counting subgroups.
    ///
    /// Asked before a deletion is confirmed, because the count is the whole
    /// content of the warning.
    pub fn group_entry_count(&self, uuid: &str) -> usize {
        entries::list(&self.vault, Some(uuid), None).len()
    }

    pub fn set_expiry(&mut self, uuid: &str, at_unix: Option<i64>) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        let encoding = self.vault.timestamp_encoding();
        let stamp = at_unix.map(|s| vardo_vault::Timestamp::from_unix(s, encoding));
        self.vault.update_entry(id, |e| e.set_expiry(stamp))?;
        self.dirty = true;
        Ok(())
    }

    pub fn set_tags(&mut self, uuid: &str, tags: &[String]) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        self.vault.update_entry(id, |e| e.set_tags(tags))?;
        self.dirty = true;
        Ok(())
    }

    /// Makes an earlier version current again: its fields come back, and
    /// fields it did not have go. An ordinary edit, so the version being
    /// replaced goes into history and the restore can itself be undone.
    ///
    /// `index` counts as [`entries::history`] does.
    pub fn restore_version(&mut self, uuid: &str, index: usize) -> Result<(), CoreError> {
        let missing = || CoreError::NotFound {
            path: uuid.to_string(),
        };
        let entry = entries::find(&self.vault, uuid).ok_or_else(missing)?;
        let old = entry.history().into_iter().nth(index).ok_or_else(missing)?;
        let restored = old.fields();
        let mut edits: Vec<FieldEdit> = restored
            .iter()
            .map(|(key, field)| FieldEdit {
                key: key.clone(),
                value: Some(field.expose().to_string()),
                protected: field.is_protected(),
            })
            .collect();
        for (key, _) in entry.fields() {
            if !restored.iter().any(|(k, _)| *k == key) {
                edits.push(FieldEdit {
                    key,
                    value: None,
                    protected: false,
                });
            }
        }
        self.update_entry(uuid, &edits)?;
        Ok(())
    }
}

impl Session {
    /// Attaches a file from disk.
    ///
    /// Read here rather than in the frontend, so the bytes never become a
    /// JavaScript string. A recovery-codes file base64'd through the IPC
    /// boundary would sit in the WebView's heap for as long as the page lives.
    pub fn add_attachment(&mut self, uuid: &str, path: &Path) -> Result<String, CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;

        let data = std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::NotFound {
                path: path.display().to_string(),
            },
            _ => CoreError::Io {
                detail: e.to_string(),
            },
        })?;

        // A vault is not a file share, and every byte here is re-encrypted on
        // every save and re-uploaded in full on every sync. Refusing early is
        // kinder than a save that takes a minute and a sync that never
        // finishes.
        const MAX: usize = 8 * 1024 * 1024;
        if data.len() > MAX {
            return Err(CoreError::Unsupported {
                detail: format!(
                    "That file is {} MB. Vardo attaches up to 8 MB.",
                    data.len() / 1_048_576
                ),
            });
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".into());

        self.vault.add_attachment(id, &name, &data)?;
        self.dirty = true;
        Ok(name)
    }

    pub fn remove_attachment(&mut self, uuid: &str, name: &str) -> Result<(), CoreError> {
        let id = entries::parse_uuid(uuid).ok_or(CoreError::NotFound {
            path: uuid.to_string(),
        })?;
        self.vault.remove_attachment(id, name)?;
        self.dirty = true;
        Ok(())
    }
}
