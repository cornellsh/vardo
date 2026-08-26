//! DOM navigation by UUID.
//!
//! Everything here operates on a **plaintext** DOM (see
//! `vardo_vault::Vault`). Restructuring a ciphertext document shifts keystream
//! positions and corrupts every protected value after the edit, so these
//! helpers must never be pointed at one.
//!
//! Searches locate a child-index path immutably and then navigate to it,
//! rather than returning `&mut` from a recursive search — the borrow checker
//! cannot see that such a search only borrows one branch.

use vardo_kdbx::xml::{Element, Node};
use vardo_vault::Uuid;

/// Child indices from a starting element down to a match.
pub type Path = Vec<usize>;

pub fn uuid_of(e: &Element) -> Option<Uuid> {
    Uuid::parse(&e.child("UUID")?.text())
}

fn find_path(root: &Element, name: &str, uuid: Uuid) -> Option<Path> {
    for (i, child) in root.children.iter().enumerate() {
        if let Node::Element(e) = child {
            if e.name == name && uuid_of(e) == Some(uuid) {
                return Some(vec![i]);
            }
        }
    }
    for (i, child) in root.children.iter().enumerate() {
        if let Node::Element(g) = child {
            if g.name == "Group" {
                if let Some(mut rest) = find_path(g, name, uuid) {
                    rest.insert(0, i);
                    return Some(rest);
                }
            }
        }
    }
    None
}

pub fn nav_mut<'a>(root: &'a mut Element, path: &[usize]) -> &'a mut Element {
    let mut current = root;
    for &i in path {
        current = match &mut current.children[i] {
            Node::Element(e) => e,
            _ => panic!("path points at a non-element"),
        };
    }
    current
}

pub fn entry_mut(root: &mut Element, uuid: Uuid) -> Option<&mut Element> {
    let path = find_path(root, "Entry", uuid)?;
    Some(nav_mut(root, &path))
}

pub fn group_mut(root: &mut Element, uuid: Uuid) -> Option<&mut Element> {
    if uuid_of(root) == Some(uuid) {
        return Some(root);
    }
    let path = find_path(root, "Group", uuid)?;
    Some(nav_mut(root, &path))
}

pub fn find_entry(root: &Element, uuid: Uuid) -> Option<&Element> {
    for child in root.children.iter() {
        if let Node::Element(e) = child {
            if e.name == "Entry" && uuid_of(e) == Some(uuid) {
                return Some(e);
            }
            if e.name == "Group" {
                if let Some(found) = find_entry(e, uuid) {
                    return Some(found);
                }
            }
        }
    }
    None
}

pub fn find_group(root: &Element, uuid: Uuid) -> Option<&Element> {
    if uuid_of(root) == Some(uuid) {
        return Some(root);
    }
    for child in root.children.iter() {
        if let Node::Element(g) = child {
            if g.name == "Group" {
                if let Some(found) = find_group(g, uuid) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Removes an entry and hands it back, so a move can reinsert it elsewhere.
pub fn take_entry(root: &mut Element, uuid: Uuid) -> Option<Node> {
    let path = find_path(root, "Entry", uuid)?;
    let (last, parents) = path.split_last().expect("path is never empty");
    let parent = nav_mut(root, parents);
    Some(parent.children.remove(*last))
}

pub fn take_group(root: &mut Element, uuid: Uuid) -> Option<Node> {
    let path = find_path(root, "Group", uuid)?;
    let (last, parents) = path.split_last().expect("path is never empty");
    let parent = nav_mut(root, parents);
    Some(parent.children.remove(*last))
}

/// The UUID of the group directly containing `uuid`, if any.
pub fn parent_of(root: &Element, uuid: Uuid) -> Option<Uuid> {
    for child in root.children.iter() {
        if let Node::Element(e) = child {
            if (e.name == "Entry" || e.name == "Group") && uuid_of(e) == Some(uuid) {
                return uuid_of(root);
            }
        }
    }
    for child in root.children.iter() {
        if let Node::Element(g) = child {
            if g.name == "Group" {
                if let Some(found) = parent_of(g, uuid) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Appends a child element, un-collapsing `<Foo/>` into `<Foo>...</Foo>` and
/// matching the surrounding indentation so the result still looks hand-written.
pub fn append_child(parent: &mut Element, child: Element) {
    let indent = detect_indent(parent);
    parent.self_closing = false;

    // Insert before any trailing whitespace so the closing tag stays aligned.
    let insert_at = parent
        .children
        .iter()
        .rposition(|n| !matches!(n, Node::Text(t) if t.trim().is_empty()))
        .map(|i| i + 1)
        .unwrap_or(parent.children.len());

    if let Some(indent) = indent {
        parent.children.insert(insert_at, Node::Text(indent));
        parent.children.insert(insert_at + 1, Node::Element(child));
    } else {
        parent.children.insert(insert_at, Node::Element(child));
    }
}

/// The whitespace text node that precedes existing element children, so new
/// siblings can be indented the same way.
fn detect_indent(parent: &Element) -> Option<String> {
    let mut previous_text: Option<&String> = None;
    for child in &parent.children {
        match child {
            Node::Text(t) if t.trim().is_empty() => previous_text = Some(t),
            Node::Element(_) => return previous_text.cloned(),
            _ => previous_text = None,
        }
    }
    None
}

/// Removes an element's trailing whitespace-only text node.
///
/// After deleting the last element from a group, the leftover indentation
/// would otherwise leave `<Group>\n\t\t</Group>` with a stray blank run.
pub fn tidy(parent: &mut Element) {
    while matches!(parent.children.last(), Some(Node::Text(t)) if t.trim().is_empty())
        && parent
            .children
            .iter()
            .filter(|n| matches!(n, Node::Element(_)))
            .count()
            == 0
    {
        parent.children.pop();
    }
}
