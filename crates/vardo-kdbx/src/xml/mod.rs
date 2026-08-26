//! A DOM that round-trips byte-for-byte.
//!
//! # Why this exists
//!
//! Every other Rust KDBX library parses XML into a closed typed model and
//! serialises back out of it. Anything the model does not name is destroyed.
//! Measured against real KeePass files, that costs icon assignments, fabricates
//! `QualityCheck` elements, and rewrites 22% of the document (see
//! `PHASE0-FINDINGS.md`).
//!
//! For a password manager that is merely bad. For one whose whole value is a
//! **merge engine**, it is fatal: a save that changes bytes it was not asked to
//! change manufactures phantom conflicts on every sync.
//!
//! So the rule here is absolute:
//!
//! > **Parse everything. Understand what you can. Re-emit the rest untouched.**
//!
//! # How byte-stability is achieved
//!
//! Four things that naive DOMs discard, and which we keep:
//!
//! 1. **Self-closing form.** `<Tags/>` and `<Tags></Tags>` are semantically
//!    identical and textually different. We remember which one was there.
//! 2. **Attribute order and quote character.** Attributes are a `Vec`, not a
//!    map, and each remembers whether it was quoted with `"` or `'`.
//! 3. **Raw text.** Text is stored *as it appeared*, still escaped. Decoding
//!    happens on access, encoding only on mutation. An untouched value is
//!    therefore re-emitted as the exact bytes we read, which sidesteps the
//!    entire problem of guessing a writer's escaping style (`&#xD;` vs `&#13;`
//!    vs a literal CR).
//! 4. **Inter-element whitespace.** Indentation is just text nodes, and we keep
//!    them.

mod escape;
mod parse;
mod write;

pub use escape::{decode_text, encode_text};
pub use parse::{parse, ParseLimits};

/// Which quote character delimited an attribute value in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    Double,
    Single,
}

impl Quote {
    pub fn ch(self) -> char {
        match self {
            Quote::Double => '"',
            Quote::Single => '\'',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    /// Still-escaped value, exactly as it appeared in the source.
    pub raw_value: String,
    pub quote: Quote,
    /// Whitespace that separated this attribute from what preceded it.
    ///
    /// Writers are not required to use a single space, and a document using
    /// anything else would be rewritten on save if we normalised here.
    pub leading_space: String,
    /// Whitespace between the attribute name and `=`, as in `x ="1"`.
    pub space_before_eq: String,
    /// Whitespace between `=` and the value, as in `x= "1"`.
    pub space_after_eq: String,
}

impl Attribute {
    pub fn new(name: impl Into<String>, value: &str) -> Self {
        Self {
            name: name.into(),
            raw_value: encode_text(value, true),
            quote: Quote::Double,
            leading_space: " ".to_string(),
            space_before_eq: String::new(),
            space_after_eq: String::new(),
        }
    }

    pub fn value(&self) -> String {
        decode_text(&self.raw_value)
    }

    pub fn set_value(&mut self, value: &str) {
        self.raw_value = encode_text(value, true);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    /// Character data, still escaped. Includes indentation whitespace.
    Text(String),
    CData(String),
    Comment(String),
    /// A processing instruction other than the XML declaration, e.g. `<?foo?>`.
    ProcessingInstruction(String),
}

impl Node {
    pub fn as_element(&self) -> Option<&Element> {
        match self {
            Node::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_element_mut(&mut self) -> Option<&mut Element> {
        match self {
            Node::Element(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
    /// True if the source wrote `<Foo/>` rather than `<Foo></Foo>`.
    ///
    /// Only meaningful when `children` is empty. Preserving this is what stops
    /// us from silently rewriting the ~35 empty elements in a typical KeePass
    /// document on every save.
    pub self_closing: bool,
    /// Whitespace between the last attribute (or the name) and the closing
    /// `>` or `/>`.
    ///
    /// Real writers disagree here: KeePassXC emits `<Tags/>` while IntelliJ's
    /// KeePass library emits `<Tags />`. Normalising it would rewrite every
    /// empty element in a file produced by the other tool — exactly the class
    /// of spurious diff this DOM exists to prevent.
    pub trailing_space: String,
    /// Whitespace inside the end tag, as in `</Foo >`.
    ///
    /// Legal XML and emitted by some writers. Same reasoning as
    /// `trailing_space`: normalising it rewrites the file.
    pub end_trailing_space: String,
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            children: Vec::new(),
            self_closing: true,
            trailing_space: String::new(),
            end_trailing_space: String::new(),
        }
    }

    pub fn attr(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }

    pub fn attr_value(&self, name: &str) -> Option<String> {
        self.attr(name).map(|a| a.value())
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        match self.attributes.iter_mut().find(|a| a.name == name) {
            Some(a) => a.set_value(value),
            None => self.attributes.push(Attribute::new(name, value)),
        }
    }

    pub fn remove_attr(&mut self, name: &str) -> Option<Attribute> {
        let i = self.attributes.iter().position(|a| a.name == name)?;
        Some(self.attributes.remove(i))
    }

    /// Direct child elements with the given name.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children
            .iter()
            .filter_map(Node::as_element)
            .filter(move |e| e.name == name)
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children
            .iter()
            .filter_map(Node::as_element)
            .find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children
            .iter_mut()
            .filter_map(Node::as_element_mut)
            .find(|e| e.name == name)
    }

    /// Concatenated, entity-decoded text of direct `Text`/`CData` children.
    ///
    /// Nested elements are not descended into: in KDBX, an element either holds
    /// text or holds elements, never a meaningful mix.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => out.push_str(&decode_text(t)),
                Node::CData(t) => out.push_str(t),
                _ => {}
            }
        }
        out
    }

    /// Replaces this element's content with a single escaped text node.
    ///
    /// Setting the empty string collapses the element back to `<Foo/>`, which
    /// is what KeePass writers emit for empty values.
    pub fn set_text(&mut self, value: &str) {
        self.children.clear();
        if value.is_empty() {
            self.self_closing = true;
        } else {
            self.self_closing = false;
            self.children.push(Node::Text(encode_text(value, false)));
        }
    }

    /// Depth-first iterator over this element and all descendants.
    pub fn descendants(&self) -> Descendants<'_> {
        Descendants { stack: vec![self] }
    }
}

pub struct Descendants<'a> {
    stack: Vec<&'a Element>,
}

impl<'a> Iterator for Descendants<'a> {
    type Item = &'a Element;

    fn next(&mut self) -> Option<Self::Item> {
        let e = self.stack.pop()?;
        // Reversed so that iteration order matches document order.
        for c in e.children.iter().rev() {
            if let Node::Element(c) = c {
                self.stack.push(c);
            }
        }
        Some(e)
    }
}

/// A whole XML document, including everything outside the root element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// The `<?xml ... ?>` declaration verbatim, without the delimiters.
    pub declaration: Option<String>,
    /// Comments, PIs and whitespace before the root element.
    pub prologue: Vec<Node>,
    pub root: Element,
    /// Anything after the root element, usually a trailing newline.
    pub epilogue: Vec<Node>,
}

impl Document {
    /// Serialises back to bytes. For an unmodified document this is
    /// byte-identical to the input; `tests/roundtrip.rs` asserts that across
    /// the whole fixture corpus.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write::write_document(self, &mut out);
        out
    }
}
