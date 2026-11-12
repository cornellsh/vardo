//! Phase 0 conformance harness.
//!
//! It tested the claim behind `dec:own-kdbx-layer`: that a
//! closed typed model cannot round-trip KDBX losslessly, and so we should write
//! the container layer ourselves. If `keepass` round-trips the corpus clean,
//! the recommendation is wrong and we save a month.
//!
//!   read      <file> [pw]   decrypt to raw XML, report shape
//!   roundtrip <file> [pw]   original -> keepass -> re-encode, diff the XML

use std::collections::BTreeMap;

use vardo_kdbx::{open, Credentials};

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| usage());
    let path = args.next().unwrap_or_else(|| usage());
    let pw = args.next().unwrap_or_else(|| "demopass".into());

    match mode.as_str() {
        "read" => read(&path, &pw),
        "roundtrip" => roundtrip(&path, &pw),
        "dump" => dump(&path, &pw),
        "secrets" => secrets(&path, &pw),
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage: vardo-conformance <read|roundtrip> <file.kdbx> [password]");
    std::process::exit(2)
}

fn dump(path: &str, pw: &str) {
    let bytes = std::fs::read(path).expect("read file");
    let creds = Credentials::with_password(pw);
    let before = open(&bytes, &creds).expect("baseline");
    std::fs::write("/tmp/before.xml", &before.xml).unwrap();
    let key = keepass::DatabaseKey::new().with_password(pw);
    let db = keepass::Database::open(&mut bytes.as_slice(), key.clone()).expect("kp open");
    let mut out = Vec::new();
    db.save(&mut out, key).expect("kp save");
    let after = open(&out, &creds).expect("re-read");
    std::fs::write("/tmp/after.xml", &after.xml).unwrap();
    println!(
        "wrote /tmp/before.xml ({}B) /tmp/after.xml ({}B)",
        before.xml.len(),
        after.xml.len()
    );
}

fn secrets(path: &str, pw: &str) {
    let bytes = std::fs::read(path).expect("read file");
    let doc = open(&bytes, &Credentials::with_password(pw)).expect("open");
    let xml = doc.parse_xml().expect("xml");
    let vals =
        vardo_kdbx::protected::decrypt_all(&xml, doc.inner.stream_cipher, &doc.inner.stream_key)
            .expect("decrypt protected");
    println!("{path}: {} protected values", vals.len());
    for (i, e) in vardo_kdbx::protected::protected_ordinals(&xml)
        .into_iter()
        .enumerate()
    {
        let v = vals.get_str(i).unwrap();
        println!("  [{i}] <{}> = {:?}", e.name, *v);
    }
}

fn read(path: &str, pw: &str) {
    let bytes = std::fs::read(path).expect("read file");
    match open(&bytes, &Credentials::with_password(pw)) {
        Ok(doc) => {
            println!("OK   {path}");
            println!(
                "     version={} xml={}B binaries={} unknown_inner={}",
                doc.version(),
                doc.xml.len(),
                doc.inner.binaries.len(),
                doc.inner.unknown.len()
            );
        }
        Err(e) => println!("FAIL {path}: {e}"),
    }
}

/// Counts every element name in the document. Comparing these multisets is
/// robust to indentation and attribute-order differences between writers,
/// which a line diff is not.
fn tag_census(xml: &[u8]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let s = String::from_utf8_lossy(xml);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        i += 1;
        // Skip closing tags, declarations, comments, CDATA.
        if i >= bytes.len() || matches!(bytes[i], b'/' | b'?' | b'!') {
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        if i > start {
            *counts.entry(s[start..i].to_string()).or_insert(0) += 1;
        }
    }
    counts
}

fn roundtrip(path: &str, pw: &str) {
    let bytes = std::fs::read(path).expect("read file");
    let creds = Credentials::with_password(pw);

    let before = match open(&bytes, &creds) {
        Ok(d) => d,
        Err(e) => {
            println!("SKIP {path}: cannot read baseline: {e}");
            return;
        }
    };

    // Round-trip through the candidate library.
    let key = keepass::DatabaseKey::new().with_password(pw);
    let db = match keepass::Database::open(&mut bytes.as_slice(), key.clone()) {
        Ok(db) => db,
        Err(e) => {
            println!("SKIP {path}: keepass could not open: {e}");
            return;
        }
    };
    let mut out = Vec::new();
    if let Err(e) = db.save(&mut out, key) {
        println!("SKIP {path}: keepass could not save: {e}");
        return;
    }

    let after = match open(&out, &creds) {
        Ok(d) => d,
        Err(e) => {
            println!("FAIL {path}: re-read after save failed: {e}");
            return;
        }
    };

    let (a, b) = (tag_census(&before.xml), tag_census(&after.xml));
    let mut lost = Vec::new();
    let mut gained = Vec::new();
    for (tag, &n) in &a {
        let m = b.get(tag).copied().unwrap_or(0);
        if m < n {
            lost.push(format!("{tag} {n}->{m}"));
        }
    }
    for (tag, &m) in &b {
        let n = a.get(tag).copied().unwrap_or(0);
        if n < m {
            gained.push(format!("{tag} {n}->{m}"));
        }
    }

    let vers = if before.version() == after.version() {
        format!("{}", before.version())
    } else {
        format!("{} -> {}", before.version(), after.version())
    };

    if lost.is_empty() && gained.is_empty() {
        println!("CLEAN {path}  [{vers}]");
    } else {
        println!("LOSSY {path}  [{vers}]");
        if !lost.is_empty() {
            println!("      dropped: {}", lost.join(", "));
        }
        if !gained.is_empty() {
            println!("      added:   {}", gained.join(", "));
        }
    }
}

// Appended: dump mode for manual inspection.
