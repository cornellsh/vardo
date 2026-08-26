# Phase 0 findings — library decision

**Question:** can we depend on the `keepass` crate, or must we write the KDBX
container layer ourselves?

**Method:** built a minimal KDBX4 reader (`bolthole-kdbx`) that decrypts to raw
XML, giving ground truth independent of any candidate library. Then for each
fixture: `original → keepass::open → keepass::save → our reader → diff the XML`.

**Corpus:** 30 MIT-licensed fixtures from `sseemayer/keepass-rs`, covering
KDBX 3.1/4.0/4.1, AES/ChaCha20/Twofish, Argon2d/Argon2id/AES-KDF, key files,
TOTP, deleted entries, custom icons, KDBX 4.1 features.

**Verdict: write it ourselves. Confirmed — but for sharper reasons than I
predicted, and one of my predictions was wrong.**

---

## Result 1 — `keepass` cannot write KDBX 4.0 at all

Every 4.0 fixture failed with `Unsupported database version` on save.

```
SKIP test_db_kdbx4_with_password_argon2.kdbx        keepass could not save
SKIP test_db_kdbx4_with_password_argon2id.kdbx      keepass could not save
SKIP test_db_kdbx4_with_password_argon2_chacha20.kdbx    keepass could not save
SKIP test_db_kdbx4_with_password_argon2id_chacha20.kdbx  keepass could not save
SKIP test_db_kdbx4_with_password_deleted_entry.kdbx keepass could not save
```

This is the 2026 breaking change (`feat!: add support for KDBX4.1`) landing
exactly where it hurts: the library now *always* writes 4.1 and errors if asked
for 4.0.

**Andreas's vault is KDBX 4.0.** Using this crate would mean force-migrating the
file format on first save — violating the "preserve input version" decision in
`CRITIQUE.md` §4. Disqualifying on its own.

## Result 2 — round-trip is not byte-stable

For the files that *could* be saved (4.1), the XML shrank **20,720 B → 16,218 B**,
a 22% structural rewrite of an unmodified document.

Precise accounting of what changed:

| Element | Before | After | What happened |
|---|---|---|---|
| `<Tags/>` | 8 empty | 0 | empty elements dropped |
| `<Notes/>` | 2 empty | 0 | empty elements dropped |
| `<BackgroundColor/>` | 8 empty | 0 | empty elements dropped |
| `<ForegroundColor/>` | 8 empty | 0 | empty elements dropped |
| `<OverrideURL/>` | 8 empty | 0 | empty elements dropped |
| `<DefaultSequence/>` | 8 empty | 0 | empty elements dropped |
| `<DatabaseName/>`, `<DatabaseDescription/>`, `<DefaultUserName/>`, `<Color/>` | 1 each | 0 | empty elements dropped |
| `<IconID>` | 11 | 9 | **2 icon assignments lost** |
| `<CustomIconUUID>` | 2 | 1 | **1 custom icon assignment lost** |
| `<QualityCheck>` | 1 | 8 | **7 injected that were never there** |

## Result 3 — where I was wrong

In `CRITIQUE.md` §2.2 I implied unmodelled elements would silently eat user
content. **On this corpus, no text content was lost.** I checked directly:

```
Tags with content lost:   (none)
Notes with content lost:  (none)
```

`<Notes>this group has tags</Notes>`, `<Tags>a;b;c</Tags>`, `CustomData`,
`KPRPC JSON` blobs, embedded PNG custom icons, entry history, `DeletedObjects`,
`PreviousParentGroup` — all survived intact. The crate models more than I gave
it credit for, and it is genuinely good at what it's built for.

**Correcting the record: this is not a "it eats your passwords" result.** It's a
normalisation result. That is a weaker claim than I made, and I should have
verified before writing it.

## Result 4 — why normalisation is still disqualifying *for this product*

Dropping `<Tags/>` in favour of no element is arguably semantically identical.
It is still fatal here, for a reason specific to what Bolthole is:

> **We are building a merge engine. A non-byte-stable round-trip manufactures
> phantom diffs on every single save.**

KeePassXC writes the file. Bolthole opens it, changes *nothing*, saves. The
document now differs by ~35 elements. Any structural 3-way merge sees that as
change on our side and raises conflicts that do not exist.

This is not hypothetical — it is **KeePassXC issue #3340**, "conflicting changes
on identical databases," which their own maintainers have never fully fixed.
Building on a normalising round-trip would bake that bug into the foundation of
the one subsystem that is supposed to be our differentiator (`CRITIQUE.md` §3.6).

And the injections cut deeper than the drops. `QualityCheck` appearing on 7
entries that never had it is not normalisation — it's fabricating state. Losing
2 `IconID`s and a `CustomIconUUID` is straightforward user-visible data loss.

## Result 5 — the architectural point survives

None of the above is a bug to file. It's the closed typed model in
`src/db/types/entry.rs` doing exactly what it was designed to do: parse into
named fields, serialise from named fields. Everything not named is not
represented, so it cannot be written back.

The recent commit stream (`preserve empty fields as is`, `preserve group and
entry insertion order`, `correct several cross-library compatibility issues`)
is the maintainer patching this symptom-by-symptom. Fidelity is being pursued as
a series of bug fixes rather than guaranteed by construction — which is the
distinction `CRITIQUE.md` §2.2 drew, and it holds.

---

## Decision

**Write `bolthole-kdbx` ourselves, DOM-first.** Confirmed by evidence, not
preference.

`keepass` stays in the workspace as a **differential-testing oracle** — where
we disagree, one of us is wrong and it's worth knowing which. It is MIT, mature,
364k downloads, and better at its actual job than anything else in Rust.

## What already works

`crates/bolthole-kdbx` reads KDBX 4.0 and 4.1 to raw XML:

```
OK  test_db_kdbx41_features.kdbx              4.1  5987 B
OK  test_db_kdbx41_with_password_aes.kdbx     4.1 20720 B
OK  test_db_kdbx4_with_password_aes.kdbx      4.1  7015 B
OK  test_db_kdbx4_with_password_argon2.kdbx   4.0  5732 B
OK  test_db_kdbx4_with_password_argon2id.kdbx 4.0  5114 B
OK  test_db_kdbx4_with_password_argon2_chacha20.kdbx    4.0 4694 B
OK  test_db_kdbx4_with_password_argon2id_chacha20.kdbx  4.0 3936 B
OK  test_db_kdbx4_with_password_deleted_entry.kdbx      4.0 7073 B
```

9/9 password-protected KDBX4 fixtures. Covers AES-256-CBC and ChaCha20 outer
ciphers, Argon2d/Argon2id/AES-KDF, gzip, full header-SHA256 + header-HMAC +
per-block HMAC authentication, and inner-header parsing with unknown-field
retention.

Not yet: Twofish (2 fixtures), key files (3 fixtures — deferred, password-only
per instruction), KDBX 3.1 (deliberately erroring rather than mishandled).

Measured: **0.871 s** for Argon2d m=64 MiB t=14 p=2 on this i7-12700H, matching
the vault's real parameters. Async unlock with progress is mandatory, confirmed.

## Next

1. XML layer — DOM-preserving parse/serialise, byte-stable round-trip as a
   property test against the whole corpus.
2. Writer — container encode, `preserve input version`, differential-test every
   output against `keepass` *and* `keepassxc-cli`.
3. `cargo-fuzz` on the parser before any of it touches a real file.
