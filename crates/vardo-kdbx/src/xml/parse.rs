//! A strict, allocation-light XML parser for the KDBX payload.
//!
//! Hand-written rather than pulled from a crate for two reasons. First, no
//! general-purpose parser preserves self-closing form, attribute quote style
//! and raw entity text, which is precisely what byte-stability needs. Second,
//! the attack surface here is a file that may have come from anywhere, so the
//! feature set is deliberately tiny: no DTDs, no entity definitions, no
//! namespace processing, no external anything.

use super::{Attribute, Document, Element, Node, Quote};
use crate::error::{Error, Result};

/// Resource ceilings applied while parsing.
///
/// A `.kdbx` payload is HMAC-authenticated before it reaches the parser, so a
/// hostile document implies a hostile *writer* — someone who handed you a file,
/// or a synced vault from a compromised account. These bounds mean the worst
/// case is a clean error rather than an out-of-memory kill.
#[derive(Debug, Clone, Copy)]
pub struct ParseLimits {
    /// Guards against stack exhaustion from deeply nested elements.
    pub max_depth: usize,
    /// Guards against documents with millions of tiny elements.
    pub max_elements: usize,
    pub max_attributes_per_element: usize,
    /// Guards against a single pathological attribute or text run.
    pub max_name_len: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            // Real vaults nest a handful of groups deep; 256 is far beyond any
            // legitimate document while still being cheap to unwind.
            max_depth: 256,
            max_elements: 5_000_000,
            max_attributes_per_element: 256,
            max_name_len: 1024,
        }
    }
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    limits: ParseLimits,
    elements_seen: usize,
}

pub fn parse(bytes: &[u8], limits: ParseLimits) -> Result<Document> {
    let src = std::str::from_utf8(bytes).map_err(|e| Error::Xml {
        offset: e.valid_up_to(),
        detail: "payload is not valid UTF-8".into(),
    })?;
    Parser {
        src,
        pos: 0,
        limits,
        elements_seen: 0,
    }
    .document()
}

impl<'a> Parser<'a> {
    fn err<T>(&self, detail: impl Into<String>) -> Result<T> {
        Err(Error::Xml {
            offset: self.pos,
            detail: detail.into(),
        })
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn starts_with(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    /// Skips XML whitespace, which is exactly `#x20 | #x9 | #xD | #xA`.
    ///
    /// Deliberately *not* `str::trim_start`, which also strips vertical tab,
    /// form feed and assorted Unicode spaces. Being more permissive than the
    /// spec is a parser-differential hazard: we would accept documents other
    /// readers reject, and the same file would then mean different things to
    /// different clients.
    fn skip_whitespace(&mut self) -> &'a str {
        let start = self.pos;
        let n = self
            .rest()
            .bytes()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .count();
        self.bump(n);
        &self.src[start..self.pos]
    }

    /// Consumes up to and including `needle`, returning the text before it.
    fn take_until(&mut self, needle: &str, what: &str) -> Result<&'a str> {
        match self.rest().find(needle) {
            Some(i) => {
                let body = &self.src[self.pos..self.pos + i];
                self.bump(i + needle.len());
                Ok(body)
            }
            None => self.err(format!("unterminated {what}: no {needle:?} found")),
        }
    }

    fn document(&mut self) -> Result<Document> {
        let mut declaration = None;
        if self.starts_with("<?xml") {
            self.bump(2);
            declaration = Some(self.take_until("?>", "XML declaration")?.to_string());
        }

        let mut prologue = Vec::new();
        let root = loop {
            match self.node(0)? {
                Some(Node::Element(e)) => break e,
                Some(other) => prologue.push(other),
                None => return self.err("document has no root element"),
            }
        };

        let mut epilogue = Vec::new();
        while let Some(n) = self.node(0)? {
            if let Node::Element(e) = &n {
                return self.err(format!(
                    "second root element <{}>; XML permits exactly one",
                    e.name
                ));
            }
            epilogue.push(n);
        }

        // `node` returns `None` both at end of input and when it meets an end
        // tag it does not own. Without this check the second case would end the
        // loop early and silently discard the rest of the document: `<a/></x><b/>`
        // parsed to `<a/>`, losing `<b/>` with no error at all.
        //
        // For a vault that is data loss twice over — entries missing on open,
        // then destroyed for good on the next save. Found by `cargo-fuzz`, not
        // by inspection.
        if self.pos < self.src.len() {
            return self.err("trailing content after the root element");
        }

        Ok(Document {
            declaration,
            prologue,
            root,
            epilogue,
        })
    }

    /// Parses the next node, or `None` at end of input.
    fn node(&mut self, depth: usize) -> Result<Option<Node>> {
        if self.pos >= self.src.len() {
            return Ok(None);
        }

        if !self.starts_with("<") {
            let end = self.rest().find('<').unwrap_or(self.rest().len());
            let text = &self.src[self.pos..self.pos + end];
            self.bump(end);
            return Ok(Some(Node::Text(text.to_string())));
        }

        if self.starts_with("<!--") {
            self.bump(4);
            return Ok(Some(Node::Comment(
                self.take_until("-->", "comment")?.to_string(),
            )));
        }

        if self.starts_with("<![CDATA[") {
            self.bump(9);
            return Ok(Some(Node::CData(
                self.take_until("]]>", "CDATA section")?.to_string(),
            )));
        }

        // Refusing DOCTYPE outright is what makes XXE and entity-expansion
        // ("billion laughs") attacks structurally impossible rather than
        // merely mitigated. No legitimate KDBX writer emits one.
        if self.starts_with("<!DOCTYPE") || self.starts_with("<!doctype") {
            return self.err("DOCTYPE declarations are rejected (entity-expansion defence)");
        }

        if self.starts_with("<!") {
            return self.err("unsupported markup declaration");
        }

        if self.starts_with("<?") {
            self.bump(2);
            return Ok(Some(Node::ProcessingInstruction(
                self.take_until("?>", "processing instruction")?.to_string(),
            )));
        }

        if self.starts_with("</") {
            // The caller owns end tags; seeing one here means the document is
            // unbalanced, which `element` reports with the expected name.
            return Ok(None);
        }

        Ok(Some(Node::Element(self.element(depth)?)))
    }

    fn name(&mut self) -> Result<&'a str> {
        let start = self.pos;
        for (i, c) in self.rest().char_indices() {
            let ok = if i == 0 {
                c.is_alphabetic() || c == '_' || c == ':'
            } else {
                c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.')
            };
            if !ok {
                if i == 0 {
                    return self.err(format!("expected a name, found {c:?}"));
                }
                if i > self.limits.max_name_len {
                    return self.err("name exceeds length limit");
                }
                self.bump(i);
                return Ok(&self.src[start..self.pos]);
            }
        }
        self.err("unterminated name at end of input")
    }

    fn element(&mut self, depth: usize) -> Result<Element> {
        if depth >= self.limits.max_depth {
            return self.err(format!(
                "nesting deeper than {} elements",
                self.limits.max_depth
            ));
        }
        self.elements_seen += 1;
        if self.elements_seen > self.limits.max_elements {
            return self.err(format!("more than {} elements", self.limits.max_elements));
        }

        self.bump(1); // '<'
        let name = self.name()?.to_string();
        let mut attributes = Vec::new();
        let start_trailing_space;

        loop {
            let trailing_space = self.skip_whitespace().to_string();

            if self.starts_with("/>") {
                self.bump(2);
                return Ok(Element {
                    name,
                    attributes,
                    children: Vec::new(),
                    self_closing: true,
                    trailing_space,
                    end_trailing_space: String::new(),
                });
            }
            if self.starts_with(">") {
                self.bump(1);
                // Bound to a local, never to `self`. Parsing children recurses
                // through this same function, so shared state here would let a
                // nested element consume its parent's pending whitespace.
                start_trailing_space = trailing_space;
                break;
            }
            if self.pos >= self.src.len() {
                return self.err(format!("unterminated start tag <{name}>"));
            }
            if attributes.len() >= self.limits.max_attributes_per_element {
                return self.err(format!("<{name}> has too many attributes"));
            }
            attributes.push(self.attribute(trailing_space)?);
        }

        let mut children = Vec::new();
        let end_trailing_space;
        loop {
            if self.pos >= self.src.len() {
                return self.err(format!("unclosed element <{name}>"));
            }
            if self.starts_with("</") {
                self.bump(2);
                let close = self.name()?;
                if close != name {
                    return self.err(format!("</{close}> closes <{name}>"));
                }
                end_trailing_space = self.skip_whitespace().to_string();
                if !self.starts_with(">") {
                    return self.err(format!("malformed end tag </{close}"));
                }
                self.bump(1);
                break;
            }
            match self.node(depth + 1)? {
                Some(n) => children.push(n),
                None => return self.err(format!("unclosed element <{name}>")),
            }
        }

        Ok(Element {
            name,
            attributes,
            children,
            // `<Foo></Foo>` round-trips as written, not collapsed to `<Foo/>`.
            self_closing: false,
            trailing_space: start_trailing_space,
            end_trailing_space,
        })
    }

    fn attribute(&mut self, leading_space: String) -> Result<Attribute> {
        if leading_space.is_empty() {
            return self.err("attribute is not separated from what precedes it");
        }
        let name = self.name()?.to_string();
        let space_before_eq = self.skip_whitespace().to_string();
        if !self.starts_with("=") {
            return self.err(format!("attribute {name:?} has no value"));
        }
        self.bump(1);
        let space_after_eq = self.skip_whitespace().to_string();

        let quote = match self.peek() {
            Some('"') => Quote::Double,
            Some('\'') => Quote::Single,
            Some(c) => return self.err(format!("attribute {name:?} value starts with {c:?}")),
            None => return self.err(format!("attribute {name:?} value is truncated")),
        };
        self.bump(1);
        let delim = quote.ch().to_string();
        let raw_value = self
            .take_until(&delim, &format!("attribute {name:?} value"))?
            .to_string();

        Ok(Attribute {
            name,
            raw_value,
            quote,
            leading_space,
            space_before_eq,
            space_after_eq,
        })
    }
}
