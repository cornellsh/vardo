# Invariants

Each entry is a rule that must hold after every change.
**Rule** says what holds. **Why** says what breaks without it. **Proof** names the tests that fail if it breaks.
**Gap** marks a rule with no automated proof yet, and names the plan item that closes it.

Cite a rule in code as `inv:<name>`. The `doc-links` gate of `cargo xtask check` fails when a cited tag or a named test does not exist.

## Format

### inv:byte-stable-roundtrip

- **Rule:** Parsing XML and serialising it reproduces the input byte for byte. A save changes only the bytes it was asked to change.
- **Why:** Merge compares documents. A save that rewrites untouched bytes creates phantom changes, then phantom conflicts on every sync (`fact:keepass-crate-normalises`).
- **Proof:** `crates/vardo-kdbx/tests/xml_roundtrip.rs::xml_roundtrip_is_byte_identical`, `crates/vardo-kdbx/tests/xml_roundtrip.rs::every_whitespace_position_in_the_grammar_round_trips`, `crates/vardo-kdbx/tests/save_roundtrip.rs::payload_survives_a_save_byte_for_byte`, `crates/vardo-kdbx/tests/save_roundtrip.rs::an_edit_is_visible_and_nothing_else_moves`, and the fuzz target `fuzz/fuzz_targets/xml_roundtrip.rs`.

### inv:preserve-format-version

- **Rule:** A save keeps the version, cipher, compression and KDF the file arrived with. KDBX 3.1 opens read-only.
- **Why:** A silent upgrade can lock out an older client on another device.
- **Proof:** `crates/vardo-kdbx/tests/save_roundtrip.rs::format_settings_are_preserved_not_upgraded`, `crates/vardo-core/tests/session.rs::kdbx31_opens_but_reports_itself_unwritable`.

### inv:other-clients-open-our-files

- **Rule:** Every file Vardo writes opens in KeePassXC, and its protected values read back the same.
- **Why:** The file must outlive Vardo (`dec:kdbx-file-is-the-product`).
- **Proof:** `crates/vardo-kdbx/tests/save_roundtrip.rs::saved_files_are_accepted_by_keepassxc`, `crates/vardo-kdbx/tests/protected_values.rs::aes_kdbx41_matches_keepassxc`. Both skip without `keepassxc-cli`. CI installs it.

### inv:fresh-keys-every-save

- **Rule:** Every save draws a new master seed, IV, KDF salt and inner-stream key.
- **Why:** Reusing an inner-stream key at the same position leaks the XOR of the old and new secret.
- **Proof:** `crates/vardo-kdbx/tests/save_roundtrip.rs::every_save_produces_different_ciphertext`, `crates/vardo-kdbx/tests/save_roundtrip.rs::protected_values_survive_and_are_re_encrypted`.

### inv:hostile-files-fail-fast

- **Rule:** A malformed or malicious file errors without panicking, hanging, overflowing the stack or allocating what its header claims. KDF parameters pass `Limits` before the KDF runs. DOCTYPE and external entities are refused.
- **Why:** KDF parameters are read before anything can be authenticated (`fact:kdbx-kdf-before-auth`).
- **Proof:** `crates/vardo-kdbx/tests/adversarial.rs::argon2_memory_bomb_is_refused_without_allocating`, `crates/vardo-kdbx/tests/adversarial.rs::truncation_at_every_offset_errors_without_panicking`, `crates/vardo-kdbx/tests/adversarial.rs::single_bit_flips_are_detected_never_silently_accepted`, `crates/vardo-kdbx/tests/adversarial.rs::external_entity_reference_cannot_reach_the_filesystem`, `crates/vardo-kdbx/tests/adversarial.rs::trailing_content_is_rejected_not_silently_dropped`, and the fuzz targets in `fuzz/fuzz_targets/`.

### inv:compare-canonical-not-raw

- **Rule:** Merge and change detection compare the canonical payload, with protected values as plaintext. They never compare raw bytes.
- **Why:** Every save re-encrypts protected values, so two saves of identical data differ in every protected field.
- **Proof:** `crates/vardo-merge/tests/scenarios.rs::identical_vaults_produce_no_changes`.

## Model

### inv:plaintext-while-unlocked

- **Rule:** An unlocked `Vault` holds plaintext in its DOM and encrypts only at save. Nothing edits a ciphertext document.
- **Why:** Protected values decrypt by position in one keystream (`fact:kdbx-protected-positional`). Editing ciphertext in place once corrupted every password after a deleted entry, silently.
- **Proof:** `crates/vardo-vault/tests/edit.rs::editing_one_entry_does_not_corrupt_another_entrys_password`, `crates/vardo-vault/tests/edit.rs::every_other_entry_survives_a_save_after_a_delete`, `crates/vardo-kdbx/tests/save_roundtrip.rs::editing_a_ciphertext_document_is_documented_as_unsafe`.

### inv:edits-keep-history

- **Rule:** An edit pushes exactly one history record. A deletion goes to the recycle bin, or leaves one tombstone with its time.
- **Why:** History is where merge keeps the losing version, and tombstones are how other devices learn of a deletion.
- **Proof:** `crates/vardo-vault/tests/edit.rs::editing_pushes_exactly_one_history_record`, `crates/vardo-vault/tests/edit.rs::deleting_without_a_recycle_bin_leaves_a_tombstone`, `crates/vardo-vault/tests/edit.rs::deleting_twice_does_not_duplicate_the_tombstone`.

## Merge

### inv:merge-whole-entries

- **Rule:** The newer entry wins whole. The losing version goes into history. Fields from different versions are never combined.
- **Why:** A username from one device and a password from another can form a login that never existed.
- **Proof:** `crates/vardo-merge/tests/apply.rs::a_remote_edit_is_taken_and_the_local_version_is_kept_in_history`, `crates/vardo-merge/tests/apply.rs::both_devices_reach_the_same_content`.

### inv:merge-settles

- **Rule:** Merging the same remote a second time changes nothing. Merging A into B and B into A, both resolving conflicts toward the same copy, gives the same entries in the same groups. Keeping the other copy of a conflict takes its delete, its restore and its move, not only its fields.
- **Why:** A merge that is not idempotent drifts a little on every sync. One that depends on direction leaves two devices permanently different while both report success.
- **Proof:** `crates/vardo-merge/tests/properties.rs` checks both over 48 seeded random edit pairs (`MERGE_SEED=<n>` reruns one). `crates/vardo-merge/tests/apply.rs::keep_remote_restores_an_entry_deleted_here_and_edited_there`, `crates/vardo-merge/tests/apply.rs::keep_remote_takes_a_delete_made_there`, `crates/vardo-merge/tests/apply.rs::keep_remote_follows_a_move_made_there_in_the_same_second` and `crates/vardo-merge/tests/apply.rs::a_local_move_survives_merging_a_remote_edit_twice` pin the bugs it found.

### inv:same-second-is-conflict

- **Rule:** Divergent edits with equal timestamps are a conflict. Identical content at the same second is not.
- **Why:** KDBX stores whole seconds (`fact:kdbx-same-second`). Guessing is how vaults corrupt quietly.
- **Proof:** `crates/vardo-merge/tests/scenarios.rs::same_second_edits_are_a_conflict_not_a_coin_flip`, `crates/vardo-merge/tests/scenarios.rs::delete_and_edit_in_the_same_second_is_a_conflict`, `crates/vardo-merge/tests/scenarios.rs::same_content_at_the_same_second_is_not_a_conflict`.

### inv:no-base-no-delete

- **Rule:** Without a base, merge deletes nothing. An absence without a tombstone keeps the entry. A deletion wins only when strictly newer than the edit.
- **Why:** A first sync cannot tell "they added it" from "we deleted it". Keeping is recoverable and dropping is not.
- **Proof:** `crates/vardo-merge/tests/scenarios.rs::without_a_base_nothing_is_ever_deleted`, `crates/vardo-merge/tests/scenarios.rs::a_deletion_newer_than_the_edit_is_applied`, `crates/vardo-merge/tests/scenarios.rs::an_edit_newer_than_the_deletion_resurrects_the_entry`.

### inv:refuse-different-vaults

- **Rule:** Merging two different databases is refused, unless a caller asks for the import override by name.
- **Why:** A mistaken pick of the wrong file would otherwise merge two vaults into one.
- **Proof:** `crates/vardo-merge/tests/identity.rs::merging_two_different_databases_is_refused`, `crates/vardo-merge/tests/identity.rs::the_refusal_names_both_databases`.

### inv:plan-before-apply

- **Rule:** `plan` never mutates. The user sees what a merge will do before it happens. Every `Change` and `Conflict` variant has prose written in Rust, so a new variant without prose fails to compile.
- **Why:** A review that silently omits a change is worse than none, because it is trusted.
- **Proof:** `plan` takes shared references in `crates/vardo-merge/src/lib.rs`. `crates/vardo-merge/tests/scenarios.rs::summary_is_human_readable`.

## Storage

### inv:detect-buried-writes

- **Rule:** After a write to a backend that cannot refuse writes, check that the revision directly beneath ours was head when we wrote. If not, fold the buried write in and retry, with a bound.
- **Why:** Drive has no conditional write (`fact:drive-no-cas`). Without this check a write between our read and our write is lost.
- **Proof:** `crates/vardo-sync/tests/races.rs::a_write_that_loses_a_race_is_detected_and_redone`, `crates/vardo-sync/tests/races.rs::an_endless_racer_is_given_up_on_rather_than_looped_forever`, `crates/vardo-sync/tests/chaos.rs::devices_converge_across_many_random_interleavings`, `crates/vardo-sync/tests/chaos.rs::no_committed_password_is_ever_unreachable`, `crates/vardo-sync/tests/chaos.rs::the_remote_is_always_readable_after_chaos`.

### inv:conditional-write-when-possible

- **Rule:** A backend that can refuse a stale write overrides `write_if_unchanged` and refuses. Only backends that cannot refuse use the write-then-detect default.
- **Why:** A refused write loses nothing. A detected one still had to be repaired (`dec:conditional-write-trait`).
- **Proof:** `crates/vardo-sync/tests/conditional_write.rs::a_local_file_refuses_rather_than_burying`.

### inv:fake-drive-is-faithful

- **Rule:** `FakeDrive` models Drive's real flaws: no conditional write, revision history, a file id that can be lost, injected faults. It never offers a guarantee Drive lacks.
- **Why:** A fake with compare-and-swap would let a broken protocol pass here and lose data in production.
- **Proof:** `crates/vardo-sync/src/fake.rs`. The live contract run in `docs/RUNBOOKS.md` re-checks the facts it models.

### inv:update-in-place

- **Rule:** A Drive save patches the existing file id. It never creates a new file and deletes the old.
- **Why:** A new id kills every other client's per-file grant (`fact:drive-safe-save-kills-id`).
- **Proof:** `crates/vardo-drive/tests/backend.rs::writing_patches_the_existing_file_id`, `crates/vardo-sync/tests/races.rs::a_replaced_remote_file_is_reported_as_a_dead_reference`.

### inv:atomic-verified-save

- **Rule:** A local save writes a temp file, reopens and verifies it, then renames it over the original and keeps five backups. A failed save leaves the vault unchanged and says so. A save refuses if the file changed on disk since it was opened.
- **Why:** A crash mid-save must never cost the previous file, and a second writer must never be overwritten blind.
- **Proof:** `crates/vardo-core/tests/unhappy.rs::a_failed_save_says_the_vault_is_unchanged`, `crates/vardo-core/tests/session.rs::a_save_survives_a_reopen`, `crates/vardo-core/tests/lost_update.rs`.

### inv:warn-on-cloud-mounts

- **Rule:** A vault under a cloud mount or a sync-daemon folder gets a warning that offers migration to app-private storage.
- **Why:** Syncing a KDBX file through a mounted cloud folder is the most common way KeePass users lose data (`fact:cloud-mounts-lose-data`).
- **Proof:** `crates/vardo-sync/src/storage_risk.rs::an_rclone_mount_is_detected_by_filesystem_not_by_name`, `crates/vardo-sync/src/storage_risk.rs::a_sync_daemon_on_an_ordinary_directory_is_still_flagged`, `crates/vardo-sync/tests/races.rs::cloud_mounted_paths_are_recognised`.

## Application

### inv:secrets-stay-in-rust

- **Rule:** No secret reaches the WebView, except the one field a user explicitly reveals. View models cannot hold a protected value. `Session::field_value` is the only exit, and it returns a `Zeroizing` buffer.
- **Why:** JavaScript strings cannot be wiped, and the WebView is the largest attack surface. A WebView compromise then leaks titles and usernames, not the vault (`dec:tauri-webview-without-secrets`).
- **Proof:** `crates/vardo-core/tests/session.rs::the_serialised_entry_contains_no_secret`, `crates/vardo-core/tests/session.rs::field_value_is_how_a_secret_is_reached_deliberately`, `crates/vardo-core/tests/health.rs::the_report_never_contains_a_password`, `app/src/components/EntryDetail.dom.test.tsx`.

### inv:clipboard-written-in-rust

- **Rule:** Copying writes the clipboard from Rust and clears it after 15 seconds, only if it still holds what Vardo wrote. The interface never uses `navigator.clipboard` for a secret.
- **Why:** Routing a password through the WebView to copy it would undo `inv:secrets-stay-in-rust`. Clearing someone's later copy would destroy their data.
- **Proof:** `crates/vardo-core/tests/browser_socket.rs::copying_is_done_by_the_app_and_a_failed_copy_is_not_reported_as_copied`, `crates/vardo-core/src/clipboard.rs::keeps_what_the_user_copied_since`, `crates/vardo-core/src/clipboard.rs::leaves_an_unreadable_clipboard_alone`, `crates/vardo-core/src/clipboard.rs::copy_writes_now_and_clears_later`.

### inv:lock-drops-plaintext

- **Rule:** Locking drops the `Vault` and every derived secret. Vardo locks after five idle minutes. Unsaved changes are saved first, and a failed save keeps the vault unlocked with a message.
- **Why:** The plaintext DOM exists only while unlocked (`inv:plaintext-while-unlocked`).
- **Proof:** `crates/vardo-core/tests/browser_socket.rs::locking_takes_effect_on_the_next_request`. `app/src/hooks/useAutoLock.dom.test.tsx` locks after five idle minutes, restarts on input, and locks on waking from sleep. `app/src/lib/saveThenLock.test.ts` saves before locking and stays unlocked when the save fails.

### inv:second-window-is-told

- **Rule:** A session on a vault already unlocked in another Vardo window, in the same app directory, reports `open_elsewhere` at unlock. The lock never stops anyone opening or editing.
- **Why:** Without it the second window learns only when a save is refused, after the work is done.
- **Proof:** `crates/vardo-core/tests/session.rs::a_second_window_on_the_same_vault_is_told_at_unlock`, `crates/vardo-core/src/session/instance.rs::a_second_claim_on_the_same_vault_sees_the_first_until_it_drops`.

### inv:core-has-no-toolkit

- **Rule:** `vardo-core` depends on no UI toolkit. Shells contain no decisions.
- **Why:** Logic in a shell can only be checked by driving a window (`dec:core-decides-shells-act`).
- **Proof:** The `layers` gate fails on any edge outside the tower and on any toolkit crate in core's normal dependency graph: `xtask/src/layers.rs::an_upward_or_skipping_edge_fails`, `xtask/src/layers.rs::core_reaching_a_toolkit_fails_but_a_dev_dependency_does_not`.

### inv:ipc-types-are-generated

- **Rule:** Every type that crosses `invoke` is generated from Rust into `app/src/ipc/bindings.ts`. Every command `app/src/ipc/tauri.ts` invokes is registered in `app/src-tauri/src/lib.rs`, and the reverse. Each call passes exactly the arguments its Rust command takes.
- **Why:** A hand-kept copy drifts silently. One did: the interface sent a Drive vault as `fileId` while Rust reads `file_id`.
- **Proof:** `crates/vardo-core/src/bindings.rs::the_interface_bindings_are_current` fails on a stale file, and the `app-typecheck` gate fails on a renamed field. The `ipc-names` gate checks command and argument names: `xtask/src/ipc.rs::a_renamed_command_fails_on_both_sides`, `xtask/src/ipc.rs::a_renamed_parameter_fails`.

## Browser

### inv:extension-sees-nothing-until-asked

- **Rule:** `extension/manifest.json` requests exactly `activeTab`, `scripting` and `nativeMessaging`. It has no host permissions and no content scripts. Vardo never draws UI inside a page.
- **Why:** Injected autofill UI can be hidden and clicked through by the page (`fact:clickjacking-defcon33`).
- **Proof:** `extension/src/manifest.test.js` fails on any added permission or page-reaching key.

### inv:save-asks-before-replacing

- **Rule:** Saving a login from the browser creates an entry, or replaces the password of the login saved for exactly that site and username, and only after the person confirms. A login saved for a parent domain or sitting in the recycle bin is never changed. The replaced password stays in history.
- **Why:** A save request carries what a page typed. Overwriting a saved password without a person agreeing would let one wrong click, or a bad extension, destroy the only copy.
- **Proof:** `crates/vardo-core/tests/browser.rs::a_different_saved_password_is_replaced_only_when_the_person_confirms`, `crates/vardo-core/tests/browser.rs::a_login_saved_for_a_parent_site_is_never_overwritten`, `crates/vardo-core/tests/browser.rs::a_login_in_the_recycle_bin_is_not_updated`, and the e2e saving checks in `extension/e2e/run.mjs`.

### inv:match-in-rust

- **Rule:** URL matching happens in Rust, against the tab URL the browser reports. An exact host fills. A subdomain is listed but needs an explicit choice. Never across an https to http downgrade, never to another port, never across a public suffix.
- **Why:** Ten of eleven managers tested auto-filled subdomains, which turns script injection on any subdomain into theft of the main login (`fact:clickjacking-defcon33`).
- **Proof:** `crates/vardo-browser/src/matching.rs::https_never_downgrades_to_http`, `crates/vardo-browser/src/matching.rs::a_different_port_is_a_different_service`, `crates/vardo-browser/src/matching.rs::a_public_suffix_is_not_one_site`, `crates/vardo-browser/src/matching.rs::a_subdomain_matches_but_never_as_exact`, `crates/vardo-core/tests/browser.rs::a_subdomain_is_offered_but_marked`.

### inv:fill-rechecks-match

- **Rule:** Before returning a secret to fill, core checks the match again. The extension choosing a login does not make it a match.
- **Why:** The extension is less trusted than the app.
- **Proof:** `crates/vardo-core/tests/browser.rs::a_login_is_never_filled_into_a_site_it_does_not_belong_to`, `crates/vardo-core/tests/browser.rs::a_chosen_subdomain_login_can_be_filled`.

### inv:browser-gets-secret-only-to-fill

- **Rule:** Listing returns titles, usernames and match kind only. Copy is performed by the app. Unlocking happens only in the app window.
- **Why:** A secret that enters the browser for any other reason has nowhere safe to go.
- **Proof:** `crates/vardo-core/tests/browser.rs::the_logins_for_a_page_are_listed_without_their_passwords`, `crates/vardo-core/tests/browser.rs::copying_hands_the_password_to_vardo_not_the_browser`, `crates/vardo-core/tests/browser.rs::a_locked_vault_says_so_and_gives_nothing_away`.

### inv:never-submit

- **Rule:** Filling never submits a form, fills only the top frame, skips invisible or disabled fields, and does nothing if the tab has moved on from the page Vardo approved.
- **Why:** Hidden forms and honeypots are how pages steal autofilled credentials.
- **Proof:** `extension/src/fill.test.js`, and the e2e trap form in `extension/e2e/run.mjs`.

### inv:host-is-a-relay

- **Rule:** `vardo-browser-host` holds no vault and no key. The socket dir is mode 700 and the socket mode 600. The app refuses to start a second server over a live socket.
- **Why:** The vault must stay in one process.
- **Proof:** `crates/vardo-core/tests/browser_socket.rs`, and the e2e in `extension/e2e/run.mjs`.

### inv:reply-fits-native-messaging

- **Rule:** No reply to the extension exceeds Chrome's 1 MB message limit. Lists stop at `MAX_LOGINS`.
- **Why:** Chrome drops an oversized message, and the popup would fail with no reason.
- **Proof:** `crates/vardo-core/tests/browser.rs::a_huge_vault_does_not_produce_a_reply_chrome_would_refuse`.

### inv:protocol-has-one-truth

- **Rule:** `crates/vardo-browser/src/protocol.rs` defines the protocol. Rust writes one golden file per message to `extension/protocol/`, and the extension's message table in `extension/src/native.js` must match them. The extension turns any reply it does not know into a `version` error.
- **Why:** The extension is plain JavaScript and repeats the protocol by hand. A silent mismatch would show a blank popup.
- **Proof:** `crates/vardo-browser/tests/protocol_fixtures.rs::the_extensions_protocol_files_are_current`, and `extension/src/native.test.js`.

## Interface

### inv:text-contrast

- **Rule:** Every text and background token pair passes the contrast gate. A value changed from Telegram's records its measured ratio in `app/src/styles/tokens.css`.
- **Why:** Telegram's palette fails WCAG AA in places (`fact:telegram-palette-contrast`).
- **Proof:** `app/scripts/contrast.mjs`.

### inv:keyboard-reachable

- **Rule:** Every action is reachable from the keyboard, and the focus ring is always visible.
- **Why:** A password manager is used dozens of times a day, mostly by keyboard.
- **Proof:** The `app-lint` gate runs `jsx-a11y`'s recommended rules at error level, configured in `app/eslint.config.js`.

## Project

### inv:no-phone-home

- **Rule:** No telemetry, no CDN, no remote scripts or fonts. Site icons come from the site itself, never a third-party icon service. The WebView CSP allows only `self`, `data:` images and IPC.
- **Why:** Which sites a person uses is itself sensitive (`dec:no-server-no-telemetry`).
- **Proof:** The CSP in `app/src-tauri/tauri.conf.json`. `crates/vardo-core/src/favicon.rs`. `app/src/csp.test.ts` fails on any change to the CSP.
