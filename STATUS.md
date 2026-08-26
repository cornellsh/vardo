# Status

`vardo-kdbx` is complete and hardened. Nothing above it exists yet.

## What works

**Read** — KDBX 3.1, 4.0, 4.1. AES-256-CBC, ChaCha20, Twofish. Argon2d,
Argon2id, AES-KDF. GZip. Full authentication chain: header SHA-256, header
HMAC, per-block HMAC (4.x) or hashed block stream (3.1). Inner header with
unknown-field retention.

**Write** — KDBX 4.x, preserving the input's version, cipher, compression and
KDF. Fresh master seed, IV, KDF salt and inner stream key on every save. Files
we write are opened by `keepassxc-cli` — asserted in the test suite, not
assumed.

**Protected values** — ChaCha20 and Salsa20 inner streams, decrypted in
document order. Cross-validated against `keepassxc-cli show -a Password`,
including ordering across nested entry history.

**Byte-stable XML** — parse → serialise reproduces the input exactly, across
the whole fixture corpus and under 6.5M fuzz executions.

Deliberately not implemented: key files (password-only for now), KDBX 3.1
*writing* (the format has no authenticated integrity, and every client that
reads 3.1 also reads 4.x), ARC4 inner streams (broken since 2014).

## Numbers

| | |
|---|---|
| Tests | 65, all passing |
| Clippy | clean, `--all-targets` |
| Fuzz executions | ~14.9M across four targets, zero failures |
| Rust | 4,259 lines |
| `unsafe` | none — `#![forbid(unsafe_code)]` |

## The three decisions that shaped it

**1. The DOM is the source of truth; the typed model is a view.**

Phase 0 measured the alternative. The `keepass` crate cannot write KDBX 4.0 at
all, and its 4.1 round-trip rewrites 22% of an untouched document, loses two
`IconID`s and a `CustomIconUUID`, and fabricates seven `QualityCheck` elements.
Not from bugs — from a closed typed struct that cannot represent what it does
not name.

For a password manager that is bad. For one whose value is a **merge engine**
it is fatal: a save that changes bytes nobody asked to change manufactures
phantom conflicts on every sync. That is KeePassXC issue #3340, which their
maintainers have never fully fixed.

**2. Raw payloads are not comparable; canonical payloads are.**

Every save re-encrypts protected values under a fresh inner key, because
reusing one would reuse keystream at the same position and leak the XOR of the
old and new secret. So two files holding identical data differ in every
password field, by design.

`canonical_payload()` — the XML with protected values as plaintext — is the
comparable form, and the only thing merge may diff. This is written down in
`protected.rs` because getting it wrong is subtle and catastrophic.

**3. KDF parameters are trusted before they can be authenticated.**

The header HMAC's key comes from the KDF, whose parameters live in the header.
The ordering is forced by the format:

```text
read KDF params (attacker-controlled) → run KDF (expensive) → verify HMAC
```

And the header SHA-256 is *unkeyed*, so it stops accidental corruption and
nothing else. A file claiming `Argon2 M=2GiB, I=4294967295` will allocate two
gigabytes and grind for hours before anyone discovers it was never a vault.
`Limits` is the only available defence; `tests/adversarial.rs` builds real
bombs, repairs the digest exactly as an attacker would, and times the refusal.

## What fuzzing was worth

Four bugs, three of them silent data corruption, none caught by review. The
worst: `<a/></x><b/>` parsed as `<a/>` — a stray end tag ended the epilogue
loop and the rest of the file vanished with no error. Entries missing on open,
then destroyed permanently by the next save.

Details in `fuzz/README.md`.

## Next

1. **`vardo-vault`** — the typed view over the DOM. Entries, groups, times,
   history, attachments. Reads through the DOM, writes as surgical mutations.
2. **`vardo-merge`** — clean-room 3-way merge, `Synchronize` semantics, dry-run
   preview. Diffs canonical payloads. This is the product.
3. **`vardo-sync`** — `StorageBackend` trait, `FakeDrive` with fault injection,
   then the real Google Drive backend. See `ADR-001-sync.md`.
4. **UI** — Tauri v2 + Svelte. Only after the above are boring.

## Reading order

- `RESEARCH.md` — landscape, initial architecture
- `CRITIQUE.md` — red team of the above, with corrections
- `ADR-001-sync.md` — sync architecture (supersedes `CRITIQUE.md` §3.1)
- `PHASE0-FINDINGS.md` — why we wrote the container layer, with evidence
- `fuzz/README.md` — what fuzzing found
