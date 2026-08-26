//! The load-bearing test for this whole project.
//!
//! If XML parse -> serialise is not byte-identical, the merge engine will see
//! changes that nobody made and raise conflicts that do not exist. That is
//! KeePassXC issue #3340, and avoiding it is the reason we did not simply
//! depend on an existing crate.

use std::path::{Path, PathBuf};

use vardo_kdbx::xml::{self, ParseLimits};
use vardo_kdbx::{open, Credentials};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Fixtures that open with the password "demopass".
fn password_fixtures() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let dir = fixture_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        panic!("fixture directory missing: {}", dir.display());
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "kdbx") {
            out.push(p);
        }
    }
    out.sort();
    assert!(!out.is_empty(), "no fixtures found in {}", dir.display());
    out
}

#[test]
fn xml_roundtrip_is_byte_identical() {
    let creds = Credentials::with_password("demopass");
    let mut checked = 0;

    for path in password_fixtures() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        // Fixtures needing a key file, Twofish, or a different password are
        // simply skipped; other tests cover those paths.
        let Ok(doc) = open(&bytes, &creds) else {
            continue;
        };

        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let parsed = doc
            .parse_xml()
            .unwrap_or_else(|e| panic!("{name}: XML parse failed: {e}"));
        let reserialised = parsed.to_bytes();

        if reserialised != doc.xml {
            let (a, b) = (&doc.xml, &reserialised);
            let at = a
                .iter()
                .zip(b.iter())
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(b.len()));
            let lo = at.saturating_sub(60);
            panic!(
                "{name}: round-trip differs at byte {at} ({} in, {} out)\n  in:  {:?}\n  out: {:?}",
                a.len(),
                b.len(),
                String::from_utf8_lossy(&a[lo..(at + 60).min(a.len())]),
                String::from_utf8_lossy(&b[lo..(at + 60).min(b.len())]),
            );
        }
        checked += 1;
    }

    assert!(checked >= 8, "expected to check >=8 fixtures, got {checked}");
    eprintln!("byte-identical XML round-trip across {checked} fixtures");
}

#[test]
fn preserves_self_closing_form() {
    let src = br#"<r><a/><b></b><c d="1"/></r>"#;
    let doc = xml::parse(src, ParseLimits::default()).unwrap();
    assert_eq!(doc.to_bytes(), src);

    // The distinction is real and must survive.
    let a = doc.root.child("a").unwrap();
    let b = doc.root.child("b").unwrap();
    assert!(a.self_closing);
    assert!(!b.self_closing);
}

#[test]
fn preserves_attribute_order_and_quotes() {
    let src = br#"<r z="1" a='2' m="3"/>"#;
    let doc = xml::parse(src, ParseLimits::default()).unwrap();
    assert_eq!(doc.to_bytes(), src);
    let names: Vec<_> = doc.root.attributes.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["z", "a", "m"]);
}

#[test]
fn preserves_whitespace_comments_and_cdata() {
    let src = b"<?xml version=\"1.0\"?>\n<!--lead-->\n<r>\n\t<a>x</a>\n\t<b><![CDATA[<raw>]]></b>\n</r>\n";
    let doc = xml::parse(src, ParseLimits::default()).unwrap();
    assert_eq!(doc.to_bytes(), src);
    assert_eq!(doc.root.child("b").unwrap().text(), "<raw>");
}

#[test]
fn preserves_exotic_entity_spellings() {
    // A naive decode/re-encode would rewrite &#13; as &#xD; and break bytes.
    let src = br#"<r><a>x&#13;y</a><b>p&#xD;q</b><c>&amp;&#38;</c></r>"#;
    let doc = xml::parse(src, ParseLimits::default()).unwrap();
    assert_eq!(doc.to_bytes(), src);
    assert_eq!(doc.root.child("a").unwrap().text(), "x\ry");
    assert_eq!(doc.root.child("c").unwrap().text(), "&&");
}

#[test]
fn mutation_only_touches_the_mutated_element() {
    let src = br#"<r><a>old</a><b/><c>keep</c></r>"#;
    let mut doc = xml::parse(src, ParseLimits::default()).unwrap();
    doc.root.child_mut("a").unwrap().set_text("new");
    assert_eq!(doc.to_bytes(), br#"<r><a>new</a><b/><c>keep</c></r>"#);

    // Clearing a value collapses to the self-closing form KeePass uses.
    doc.root.child_mut("c").unwrap().set_text("");
    assert_eq!(doc.to_bytes(), br#"<r><a>new</a><b/><c/></r>"#);
}

#[test]
fn descendants_walks_in_document_order() {
    let src = br#"<r><a><b/><c/></a><d/></r>"#;
    let doc = xml::parse(src, ParseLimits::default()).unwrap();
    let names: Vec<_> = doc.root.descendants().map(|e| e.name.clone()).collect();
    assert_eq!(names, ["r", "a", "b", "c", "d"]);
}

#[test]
fn preserves_whitespace_before_the_tag_close() {
    // Regression: KeePassXC writes `<Tags/>`, IntelliJ's KeePass library writes
    // `<Tags />`. Normalising either way rewrites every empty element in a file
    // produced by the other tool, which is a phantom diff on every sync.
    // Caught by the fixture corpus, not by hand.
    for src in [
        &b"<r><a /><b/></r>"[..],
        &b"<r><a  /><b\n/></r>"[..],
        &b"<r><a x=\"1\" /></r>"[..],
        &b"<r><a >text</a></r>"[..],
        &b"<r><a\t>text</a></r>"[..],
    ] {
        let doc = xml::parse(src, ParseLimits::default()).unwrap();
        assert_eq!(
            doc.to_bytes(),
            src,
            "lost whitespace in {:?}",
            String::from_utf8_lossy(src)
        );
    }
}
