# Vardo

A KDBX-native password manager for Windows, macOS, and Linux. Syncs via Google
Drive. Stays byte-compatible with KeePassium on iOS.

**Status:** the KDBX container layer is done and hardened. Nothing is
user-facing yet. See [`STATUS.md`](STATUS.md).

Reads KDBX 3.1/4.0/4.1 (AES, ChaCha20, Twofish; Argon2d/id, AES-KDF) and writes
KDBX 4.x preserving the input's format. Files it writes are opened by
`keepassxc-cli`; passwords it decrypts match `keepassxc-cli show -a Password`.

The defining property: **parse → serialise is byte-identical.** A save must not
change bytes it was not asked to change, or the merge engine will invent
conflicts that do not exist.

65 tests · clippy clean · ~14.9M fuzz executions, zero failures · no `unsafe`

- [`STATUS.md`](STATUS.md) — what works, and the three decisions that shaped it
- [`UI-PLAN.md`](UI-PLAN.md) — what the app looks like and the order it gets built
- [`DESIGN.md`](DESIGN.md) — colour, type, icons, motion, and the constraint list
- [`RESEARCH.md`](RESEARCH.md) — landscape and initial architecture
- [`CRITIQUE.md`](CRITIQUE.md) — red team of the above; corrections and arguments
- [`ADR-001-sync.md`](ADR-001-sync.md) — sync architecture (supersedes CRITIQUE §3.1)
- [`ADR-002-ui-toolkit.md`](ADR-002-ui-toolkit.md) — Tauri vs Flutter (supersedes RESEARCH §4)
- [`ADR-003-frontend-framework.md`](ADR-003-frontend-framework.md) — Svelte vs React, two wrong reasons retracted (superseded)
- [`ADR-004-base-ui-reversal.md`](ADR-004-base-ui-reversal.md) — reversed to React 19 + Base UI, and why
- [`PHASE0-FINDINGS.md`](PHASE0-FINDINGS.md) — library decision, with evidence
- [`fuzz/README.md`](fuzz/README.md) — what fuzzing found

License: Apache-2.0 OR MIT
