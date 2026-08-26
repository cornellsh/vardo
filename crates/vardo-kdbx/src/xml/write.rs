//! Serialisation.
//!
//! Deliberately dumb: it writes back exactly what the DOM holds and invents
//! nothing. No pretty-printing, no indentation, no attribute sorting, no
//! collapsing of empty elements. Indentation survives because it is stored as
//! text nodes, not because we regenerate it.
//!
//! Any cleverness added here would show up as a phantom diff in the merge
//! engine, so there is none.

use super::{Document, Element, Node};

pub fn write_document(doc: &Document, out: &mut Vec<u8>) {
    if let Some(decl) = &doc.declaration {
        out.extend_from_slice(b"<?");
        out.extend_from_slice(decl.as_bytes());
        out.extend_from_slice(b"?>");
    }
    for n in &doc.prologue {
        write_node(n, out);
    }
    write_element(&doc.root, out);
    for n in &doc.epilogue {
        write_node(n, out);
    }
}

fn write_node(node: &Node, out: &mut Vec<u8>) {
    match node {
        Node::Element(e) => write_element(e, out),
        Node::Text(t) => out.extend_from_slice(t.as_bytes()),
        Node::CData(t) => {
            out.extend_from_slice(b"<![CDATA[");
            out.extend_from_slice(t.as_bytes());
            out.extend_from_slice(b"]]>");
        }
        Node::Comment(t) => {
            out.extend_from_slice(b"<!--");
            out.extend_from_slice(t.as_bytes());
            out.extend_from_slice(b"-->");
        }
        Node::ProcessingInstruction(t) => {
            out.extend_from_slice(b"<?");
            out.extend_from_slice(t.as_bytes());
            out.extend_from_slice(b"?>");
        }
    }
}

fn write_element(e: &Element, out: &mut Vec<u8>) {
    out.push(b'<');
    out.extend_from_slice(e.name.as_bytes());
    for a in &e.attributes {
        out.push(b' ');
        out.extend_from_slice(a.name.as_bytes());
        out.push(b'=');
        let q = a.quote.ch() as u8;
        out.push(q);
        out.extend_from_slice(a.raw_value.as_bytes());
        out.push(q);
    }

    // An element with children is never written self-closing, whatever the
    // flag says; the flag only decides the form of an *empty* element.
    if e.children.is_empty() && e.self_closing {
        out.extend_from_slice(b"/>");
        return;
    }

    out.push(b'>');
    for c in &e.children {
        write_node(c, out);
    }
    out.extend_from_slice(b"</");
    out.extend_from_slice(e.name.as_bytes());
    out.push(b'>');
}
