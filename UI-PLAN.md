# UI plan

Toolkit decided in `ADR-002` (Tauri v2) and `ADR-004` (React 19 + Base UI,
TypeScript strict, Tailwind, hand-built design system on unstyled primitives).
This is the layer above them — what gets built, in what order, and why it is
shaped this way.

---

## 1. Two surfaces, not one

Almost every interaction with a password manager is *"give me the thing for X,
now"*. That is a launcher, not a file browser. 1Password's Quick Access is the
most-used part of that product, and KeePassXC has no equivalent — its answer is
Auto-Type, which is a 2009 answer to a 2026 question.

So there are two surfaces, and the small one is the primary:

**Quick access** — global hotkey, centred overlay, one search field. Type three
letters, press Enter, the password is on your clipboard and the overlay is
gone. Target: under a second, hands never leaving the keyboard. This is ~90% of
real usage.

**Main window** — browsing, editing, attachments, history, sync review,
settings. The other 10%, and where anything that needs deliberation happens.

Both talk to the same `vardo-core`. The main window is not a superset that the
overlay is carved out of; they are separate views with separate jobs.

---

## 2. Layout

### Unlock

```
        ┌────────────────────────────────────┐
        │                                    │
        │              ▣  Vardo              │
        │                                    │
        │           passwords.kdbx           │
        │                                    │
        │   ┌────────────────────────────┐   │
        │   │ ••••••••••••••••         👁 │   │
        │   └────────────────────────────┘   │
        │                                    │
        │        [ ▓▓▓▓▓▓░░░░ ]  Unlocking   │
        │                                    │
        └────────────────────────────────────┘
```

The progress bar is not decoration. Argon2d at this vault's settings measures
**0.871 s** on a fast machine and closer to three on an old laptop. KeePassXC
freezes for that whole period, which reads as a hang. Unlocking runs on a
worker thread and reports progress.

### Main window

```
┌──────────────────────────────────────────────────────────────┐
│  🔍  Search                                          ⟳    ⚙   │
├───────────────┬───────────────────────┬──────────────────────┤
│               │                       │                      │
│  All items    │  GitHub               │  GitHub              │
│  Favourites   │  github.com · andreas │  github.com          │
│  Recent       │ ───────────────────── │                      │
│               │  Google               │  Username  andreas ⧉ │
│  ───────────  │  google.com · a@…     │  Password  ••••• 👁⧉ │
│  Personal     │ ───────────────────── │  TOTP      392 041 ⧉ │
│    Banking    │  AWS                  │  URL       …       ↗ │
│    Work       │  console.aws…         │  Notes     —         │
│  ───────────  │                       │                      │
│  Trash        │                       │  Modified 2 days ago │
│               │                       │  History  4 versions │
├───────────────┴───────────────────────┴──────────────────────┤
│  ✓  Synced · 2 minutes ago                                   │
└──────────────────────────────────────────────────────────────┘
```

Three panes because you want search results to stay visible while inspecting
an entry. Below ~1100 px the detail pane takes over the list.

Two deliberate choices:

**Search is the primary axis; the group tree is secondary.** The tree is there
because KDBX has one and people organise, but it is not the main way in. A
vault's job is retrieval, and retrieval is search.

**Copy is the primary action, reveal is secondary.** People overwhelmingly want
the value in the clipboard, not on screen. The eye icon exists but the copy
icon is the one under the cursor.

### Merge review

Nobody else ships this screen, and it is the product's differentiator, so it
gets real design rather than a modal with a warning triangle.

```
┌──────────────────────────────────────────────────────────────┐
│  Changes from another device                                 │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│  Will be applied                                             │
│    ↑  Google          password updated      from that device │
│    +  Nextcloud       new entry             from that device │
│    ✕  Old router      deleted               on that device   │
│    →  AWS             moved to Work         from that device │
│                                                              │
│  Needs you                                              (1)  │
│    ⚠  GitHub          both devices changed the password      │
│         ┌────────────────────┬────────────────────┐          │
│         │ This device        │ That device        │          │
│         │ ••••••••  2 min ago│ ••••••••  2 min ago│          │
│         └────────────────────┴────────────────────┘          │
│         ( Keep both )  ( Keep this )  ( Keep that )          │
│                                                              │
│                                    [ Cancel ]  [ Apply ]     │
└──────────────────────────────────────────────────────────────┘
```

"Both devices changed the password" rather than "conflict" — the timestamps are
identical to the second, which is *why* this cannot be decided automatically,
and the UI should say the true thing. **Keep both** is the default and is
listed first, because it is the only option that cannot lose a password.

`vardo-merge::plan()` already produces exactly this structure; the screen is a
rendering of `MergePlan`, not a reinterpretation of it.

---

## 3. The command surface *is* the security boundary

`ADR-002` accepts a WebView on the condition that it never holds secrets in
bulk. That is not a guideline to remember — it is enforced by what the commands
return.

```rust
// Reading. None of these can return a protected value.
unlock(path, password)        -> VaultSummary       // async, emits progress
lock()                        -> ()
search(query, filter)         -> Vec<EntryRow>      // title, username, url, uuid
entry(uuid)                   -> EntryDetail        // field NAMES, no secrets

// Secrets. One field, one moment, explicitly asked for.
reveal(uuid, field)           -> SecretString       // auto-hides after 30 s
copy(uuid, field)             -> ()                 // clipboard written in Rust
totp(uuid)                    -> TotpCode           // code + seconds remaining

// Writing.
update(uuid, changes) / create(group, fields) / delete(uuid) / move(uuid, group)

// Sync.
sync()                        -> SyncReport
preview()                     -> MergePlan
apply(policy)                 -> Applied
```

`search` and `entry` **cannot** carry a password — the types do not allow it.
`reveal` is the only path, one field at a time, and it is logged. So a WebView
compromise leaks entry titles and usernames, not the vault. That is a bad day,
not a catastrophe, and it is the entire basis on which Tauri was chosen.

`copy` writes the system clipboard **from Rust** and schedules the clear there
too. Never `navigator.clipboard`: routing a password through the WebView to
copy it would undo the whole arrangement for no benefit.

---

## 4. What "Signal-like" actually means here

Not the layout — a vault is a tree, a list and a detail pane, not a
conversation list. Copying Signal's shape would be cargo-culting. What is worth
copying is the restraint:

- **One accent colour**, used for exactly one thing at a time.
- **No decoration.** No gradients, no drop shadows beyond a hairline, no
  rounded-everything. Borders and spacing do the work.
- **Animation only where it explains a state change** — locking, unlocking,
  sync — and never on hover.
- **Dark mode designed first**, because this is a tool people open at 2am.
- **Density that suits scanning.** A vault is a list you sweep with your eyes;
  airy spacing makes that worse, not better.
- **Empty states that say something useful**, not illustrations.

The measurable goal: the app should look like it has been maintained for ten
years, not like it launched last week.

---

## 5. Interaction rules

- **Base UI for every interactive primitive.** Combobox for search, Dialog for
  merge review, Menu for context actions. Hand-rolling any of these means
  owning focus management and ARIA composition forever.
- **Keyboard-first.** Every action reachable without the mouse. `⌘K` command
  palette, `/` to search, `⌘C` copies the password of the selected entry,
  `⌘⇧C` the username, `Esc` locks the quick-access overlay.
- **Auto-lock** on idle timeout, on screen lock, on suspend. Locking drops the
  master key and zeroizes the plaintext DOM via `Vault::lock()`.
- **No modal dialogs for anything routine.** Confirmation lives inline.
- **Destructive actions are undoable**, not confirmed. Deleting moves to the
  recycle bin, which the merge engine already understands as distinct from an
  ordinary move.

---

## 6. Things the backend already decided that the UI must honour

| Backend fact | What the UI has to do |
|---|---|
| Argon2 measured at 0.87 s, worse on old hardware | Async unlock with real progress; never block the window |
| `Vault` holds plaintext while unlocked | `lock()` on idle, on sleep, on screen lock — not just on quit |
| Merge produces conflicts it refuses to guess at | A review screen that presents both sides honestly |
| `ConflictPolicy::Duplicate` is the safe default | "Keep both" is the default button, listed first |
| `looks_cloud_synced()` flags dangerous paths | First-open prompt offering migration to app-private storage |
| Sync can lose a race and silently recover | Show it in history, not as an alarm — it was handled |
| KDBX 3.1 is readable but not writable | Open read-only, offer a one-way upgrade, never silently convert |
| Every save mints a new inner key | Never diff raw bytes in the UI; diff canonical form |

---

## 7. Order of work

**A — Read-only.** Unlock with progress, search, list, detail, copy with
auto-clear, auto-lock. At the end of this the app is *useful*: it gets your
passwords out. It also cannot damage anything, because no write path exists
yet.

**B — Quick access.** Global hotkey, overlay, type-to-search, Enter-to-copy.
Small, and the thing that makes it a daily driver.

**C — Writing.** Edit, create, delete, move, password generator, attachments,
history browsing.

**D — Sync.** Status in the footer, the merge review screen, first-run Drive
connection via the system-browser picker, cloud-mount migration prompt.

**E — Polish.** Command palette, TOTP, password health (weak, reused, HIBP via
k-anonymity), tray, themes.

**F — Distribution.** Ed25519 updater signing, Apple notarisation ($99/yr),
Azure Trusted Signing (~$10/mo), AppImage + Flatpak + AUR.

Phase A is the one worth being slow and careful about; it establishes the
component vocabulary everything else reuses.

---

## 8. Deliberately excluded

- **Browser extension.** Speak `keepassxc-browser`'s protocol instead, so their
  mature extension drives us unmodified. ~15 message handlers versus four
  browsers times MV3 churn times two store reviews, forever.
- **Mobile.** KeePassium is the mobile app. See `ADR-002` §1 — this is also the
  argument that decided the toolkit.
- **Any server component.**
- **Teams, sharing, org features.**
