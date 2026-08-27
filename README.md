# Vardo

A KDBX-native password manager for Windows, macOS, and Linux. Stays
byte-compatible with KeePassium on iOS.

A vault can be a file on this machine or a file in Google Drive, and the two
behave the same everywhere in the app. Drive is reached through its own API
rather than a synced folder, which is what makes it possible to merge two
devices that edited at once instead of one silently overwriting the other.
Other backends are a variant in one enum, not a rewrite.

**Status:** the app reads, edits, saves, syncs and merges real vaults. The Drive
path is unit-tested and its API assumptions were verified against a live
account, but the full open-edit-save cycle over Drive has not yet been run
end to end. See [`STATUS.md`](STATUS.md) for what works and
[`ROADMAP.md`](ROADMAP.md) for what does not.

### One surprise worth knowing about Drive

Vardo asks for the `drive.file` scope, which lets it see only files it created
or that you explicitly gave it. It cannot list, read, or touch anything else in
your Drive, and it cannot be made to.

The cost of that is real and shows up immediately: a vault already sitting in
your Drive **will not appear** in Vardo's list until you add it once. The
alternative is requesting read access to your entire Drive in order to populate
a dropdown, which is not a reasonable thing for a password manager to ask.

## Running it

Linux needs `webkit2gtk-4.1` (GTK3 — Tauri does not support `webkitgtk-6.0`):

```sh
sudo pacman -S --needed webkit2gtk-4.1 base-devel librsvg    # Arch
sudo apt install libwebkit2gtk-4.1-dev build-essential librsvg2-dev  # Debian/Ubuntu
```

Then:

```sh
cd app && npm install
npm run app                  # development, with hot reload
```

To build a standalone binary that needs no dev server:

```sh
cd app && npm run app:release
../target/release/vardo-app path/to/vault.kdbx
```

A path on the command line is opened directly, so file managers can hand it
one. Without a path, use **Open another database…**.

### Try it without risking a real vault

```sh
cp fixtures/test_db_kdbx4_with_password_argon2id.kdbx /tmp/demo.kdbx
./target/release/vardo-app /tmp/demo.kdbx      # password: demopass
```

The fixtures are MIT-licensed test databases. Editing, saving and syncing all
work against them, and the sync target is the file itself — so opening a second
copy of the app, or editing with `keepassxc-cli`, exercises the merge path.

### The UI alone, no Rust

```sh
cd app && npm run build && npm run preview
```

Runs against a mock vault of 400 entries with deliberately messy data. Useful
for design work; it cannot touch a real file.

> The Vite **dev** server occasionally serves a stale or empty module after
> rapid edits, which shows up as a blank window. `rm -rf node_modules/.vite`
> fixes it. Building and previewing avoids the class entirely.

Reads KDBX 3.1/4.0/4.1 (AES, ChaCha20, Twofish; Argon2d/id, AES-KDF) and writes
KDBX 4.x preserving the input's format. Files it writes are opened by
`keepassxc-cli`; passwords it decrypts match `keepassxc-cli show -a Password`.

The defining property: **parse → serialise is byte-identical.** A save must not
change bytes it was not asked to change, or the merge engine will invent
conflicts that do not exist.

292 tests, clippy clean, ~14.9M fuzz executions with zero failures, no `unsafe`.

Run everything with `./check.sh` — fmt, clippy at `-D warnings`, the Rust
suite, the frontend typecheck, the WCAG contrast gate, and the app build.

- [`STATUS.md`](STATUS.md) — what works, and the three decisions that shaped it
- [`UI-PLAN.md`](UI-PLAN.md) — what the app looks like and the order it gets built
- [`DESIGN.md`](DESIGN.md) — colour, type, icons, motion, and the constraint list
- [`RESEARCH.md`](RESEARCH.md) — landscape and initial architecture
- [`CRITIQUE.md`](CRITIQUE.md) — red team of the above; corrections and arguments
- [`ADR-001-sync.md`](ADR-001-sync.md) — sync architecture (supersedes CRITIQUE §3.1)
- [`ADR-002-ui-toolkit.md`](ADR-002-ui-toolkit.md) — Tauri vs Flutter (supersedes RESEARCH §4)
- [`ADR-003-frontend-framework.md`](ADR-003-frontend-framework.md) — Svelte vs React, two wrong reasons retracted (superseded)
- [`ADR-004-base-ui-reversal.md`](ADR-004-base-ui-reversal.md) — reversed to React 19 + Base UI, and why
- [`ADR-005-backend-strategy.md`](ADR-005-backend-strategy.md) — Drive is a backend, not the architecture
- [`ROADMAP.md`](ROADMAP.md) — what is wrong now, what is unhandled, and the order to fix it
- [`PHASE0-FINDINGS.md`](PHASE0-FINDINGS.md) — library decision, with evidence
- [`fuzz/README.md`](fuzz/README.md) — what fuzzing found

License: Apache-2.0 OR MIT
