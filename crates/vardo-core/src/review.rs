//! Rendering a [`MergePlan`] into something a person can decide on.
//!
//! The prose lives here rather than in the UI for one reason: adding a variant
//! to `Change` or `Conflict` must be a compile error, not a silently blank row
//! in the review screen. A merge review that quietly omits a change is worse
//! than no review at all, because it is trusted.

use vardo_merge::{Change, Conflict, MergePlan, Side};

use crate::model::{ChangeView, ConflictView, SyncOutcomeKind};

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Local => "local",
        Side::Remote => "remote",
    }
}

fn side_prose(side: Side) -> &'static str {
    match side {
        Side::Local => "this device",
        Side::Remote => "the other device",
    }
}

pub fn changes(plan: &MergePlan) -> Vec<ChangeView> {
    plan.changes.iter().map(change).collect()
}

fn change(change: &Change) -> ChangeView {
    match change {
        Change::EntryAdded {
            title, path, from, ..
        } => ChangeView {
            kind: "entry-added".into(),
            title: display_title(title),
            detail: format!("Added in {} by {}", path, side_prose(*from)),
            side: Some(side_name(*from).into()),
        },

        Change::EntryUpdated {
            title,
            winner,
            fields,
            ..
        } => ChangeView {
            kind: "entry-updated".into(),
            title: display_title(title),
            // Naming the fields matters: "updated" alone does not tell you
            // whether your password is about to change.
            detail: format!(
                "{} wins for {}. The other version is kept in history.",
                capitalise(side_prose(*winner)),
                list(fields)
            ),
            side: Some(side_name(*winner).into()),
        },

        Change::EntryDeleted { title, by, .. } => ChangeView {
            kind: "entry-deleted".into(),
            title: display_title(title),
            detail: format!("Deleted by {}", side_prose(*by)),
            side: Some(side_name(*by).into()),
        },

        Change::EntryResurrected {
            title, edited_by, ..
        } => ChangeView {
            kind: "entry-resurrected".into(),
            title: display_title(title),
            detail: format!(
                "Comes back: {} edited it after the other side deleted it",
                side_prose(*edited_by)
            ),
            side: Some(side_name(*edited_by).into()),
        },

        Change::EntryMoved {
            title, to_path, by, ..
        } => ChangeView {
            kind: "entry-moved".into(),
            title: display_title(title),
            detail: format!("Moved to {} by {}", to_path, side_prose(*by)),
            side: Some(side_name(*by).into()),
        },

        Change::GroupAdded { path, from, .. } => ChangeView {
            kind: "group-added".into(),
            title: path.clone(),
            detail: format!("Group added by {}", side_prose(*from)),
            side: Some(side_name(*from).into()),
        },

        Change::GroupUpdated { path, winner, .. } => ChangeView {
            kind: "group-updated".into(),
            title: path.clone(),
            detail: format!("Group renamed; {} wins", side_prose(*winner)),
            side: Some(side_name(*winner).into()),
        },

        Change::GroupDeleted { path, by, .. } => ChangeView {
            kind: "group-deleted".into(),
            title: path.clone(),
            detail: format!("Group deleted by {}", side_prose(*by)),
            side: Some(side_name(*by).into()),
        },
    }
}

pub fn conflicts(plan: &MergePlan) -> Vec<ConflictView> {
    plan.conflicts.iter().map(conflict).collect()
}

fn conflict(conflict: &Conflict) -> ConflictView {
    match conflict {
        Conflict::ConcurrentEdit { title, fields, .. } => ConflictView {
            kind: "concurrent-edit".into(),
            title: display_title(title),
            // Explaining *why* there is no answer prevents this reading as a
            // bug. KDBX stores whole seconds, so same-second edits are real.
            detail: format!(
                "Both devices changed {} in the same second, so neither is newer.",
                list(fields)
            ),
        },

        Conflict::DeleteVersusEdit {
            title, deleted_by, ..
        } => ConflictView {
            kind: "delete-versus-edit".into(),
            title: display_title(title),
            detail: format!(
                "{} deleted this in the same second the other device edited it.",
                capitalise(side_prose(*deleted_by))
            ),
        },

        Conflict::MissingTimestamp { title, .. } => ConflictView {
            kind: "missing-timestamp".into(),
            title: display_title(title),
            detail: "This entry has no usable modification time, so it cannot be \
                     ordered against the other version."
                .into(),
        },

        Conflict::ConcurrentMove {
            title,
            local_path,
            remote_path,
            ..
        } => ConflictView {
            kind: "concurrent-move".into(),
            title: display_title(title),
            detail: format!(
                "Moved to {local_path} here and {remote_path} on the other device, at the same time."
            ),
        },
    }
}

/// Untitled entries are ordinary in real vaults, and an empty row is
/// unreviewable.
fn display_title(title: &str) -> String {
    if title.trim().is_empty() {
        "Untitled".to_string()
    } else {
        title.to_string()
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// "Password", "Password and URL", "Password, URL and Notes".
fn list(items: &[String]) -> String {
    match items {
        [] => "this entry".to_string(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {}", rest.join(", "), last),
    }
}

/// What a completed sync did, as a sentence.
pub fn describe_outcome(outcome: &vardo_sync::protocol::Outcome) -> (SyncOutcomeKind, String) {
    use vardo_sync::protocol::Outcome as O;
    use SyncOutcomeKind as K;
    match outcome {
        O::UpToDate => (K::UpToDate, "Already in sync.".into()),
        O::Pushed => (K::Pushed, "Your changes were uploaded.".into()),
        O::Created => (K::Created, "The remote file was created.".into()),
        O::Merged(applied) => (K::Merged, describe_applied(applied)),
        O::NeedsReview(plan) => (
            K::NeedsReview,
            format!(
                "{} item(s) need a decision. Nothing was uploaded.",
                plan.conflicts.len()
            ),
        ),
    }
}

/// Prose, not a `Debug` dump.
///
/// This string is the only account a user gets of what just happened to their
/// passwords, and `Applied { entries_added: 1, entries_updated: 0, .. }` is not
/// an account — it is an internal struct leaking into the one place that most
/// needs to be legible.
pub fn describe_applied(a: &vardo_merge::Applied) -> String {
    let mut parts: Vec<String> = Vec::new();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };

    for (n, verb, one, many) in [
        (a.entries_added, "added", "entry", "entries"),
        (a.entries_updated, "updated", "entry", "entries"),
        (a.entries_deleted, "deleted", "entry", "entries"),
        (a.entries_moved, "moved", "entry", "entries"),
        (a.entries_resurrected, "restored", "entry", "entries"),
        (a.groups_added, "added", "group", "groups"),
        (
            a.conflicts_duplicated,
            "kept both versions of",
            "entry",
            "entries",
        ),
    ] {
        if n > 0 {
            parts.push(format!("{verb} {}", plural(n, one, many)));
        }
    }

    let summary = match parts.as_slice() {
        [] => "Merged and uploaded; nothing changed locally.".to_string(),
        [one] => format!("Merged and uploaded: {one}."),
        [rest @ .., last] => format!("Merged and uploaded: {} and {last}.", rest.join(", ")),
    };

    if a.skipped.is_empty() {
        summary
    } else {
        // Never silent: a planned change that could not be carried out is
        // exactly the thing a user needs to know about.
        format!(
            "{summary} {} change(s) could not be applied: {}.",
            a.skipped.len(),
            a.skipped.join("; ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_read_as_english() {
        assert_eq!(list(&[]), "this entry");
        assert_eq!(list(&["Password".into()]), "Password");
        assert_eq!(list(&["Password".into(), "URL".into()]), "Password and URL");
        assert_eq!(
            list(&["Password".into(), "URL".into(), "Notes".into()]),
            "Password, URL and Notes"
        );
    }

    #[test]
    fn untitled_entries_still_have_a_reviewable_label() {
        assert_eq!(display_title(""), "Untitled");
        assert_eq!(display_title("   "), "Untitled");
        assert_eq!(display_title("GitHub"), "GitHub");
    }

    #[test]
    fn a_conflict_explains_why_it_cannot_be_decided() {
        // Without the reason, "conflict" reads as a bug in the merge rather
        // than a property of one-second timestamps.
        let view = conflict(&Conflict::ConcurrentEdit {
            uuid: vardo_vault::Uuid::from_bytes([0; 16]),
            title: "Bank".into(),
            fields: vec!["Password".into()],
        });
        assert!(view.detail.contains("same second"));
        assert!(view.detail.contains("Password"));
    }

    #[test]
    fn an_update_names_the_fields_that_change() {
        let view = change(&Change::EntryUpdated {
            uuid: vardo_vault::Uuid::from_bytes([0; 16]),
            title: "Bank".into(),
            winner: Side::Remote,
            fields: vec!["Password".into(), "URL".into()],
        });
        assert!(
            view.detail.contains("Password and URL"),
            "a review that does not say which fields change is not a review: {}",
            view.detail
        );
        assert_eq!(view.side.as_deref(), Some("remote"));
    }
}
