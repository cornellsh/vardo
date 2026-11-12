# Changelog

Notable changes to Vardo, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions will
follow [Semantic Versioning](https://semver.org) once there is a first release.

## Unreleased

Everything so far. There has not been a release yet.

### Browser extension

- Fills logins and two-factor codes in Chrome, Brave, Edge, Vivaldi and
  Chromium, from the toolbar or with Alt+Shift+L.
- Sees no page until asked to fill, and draws nothing inside pages, so the
  2025 clickjacking attacks on password managers have nothing to target.
- Vardo checks every fill belongs to the site, never fills subdomain matches
  without a choice, never downgrades https to http, skips deleted logins and
  invisible fields.
- Copying is done by Vardo, so the extension never receives the password.
- Honours the extra addresses KeePassXC and KeePassDX store in `KP2A_URL`.
- Linux and macOS for now.

### Vaults

- Opens KeePass files in the KDBX 3.1, 4.0 and 4.1 formats, and saves 4.x files
  without changing anything it was not asked to change. Files in the older 3.1
  format open read-only.
- Opens vaults from this computer or from Google Drive, and remembers recent
  ones.
- Checks every save by opening the new file before the old one is replaced, and
  keeps the last 5 versions of local files as backups.
- Refuses to overwrite a vault that changed somewhere else since it was opened.

### Sync

- Google Drive sync that combines changes from two devices instead of letting
  one overwrite the other, and shows anything ambiguous before applying it.
- Refuses to combine two different vaults by mistake.
- Signs in through the system browser, with access limited to files Vardo
  created or was given.

### Logins

- Create, edit, move and delete logins, with notes, custom fields, tags and
  expiry dates.
- Two-factor codes, a password generator, and the history of every login.
- File attachments up to 8 MB.
- Site icons fetched directly from each site, once, and stored in the vault.
- Groups that can be nested, created, renamed and deleted.
- A password health page for reused, weak, expired and old passwords.

### The app

- A three-pane layout that adapts down to a single pane on narrow windows.
- Light and dark appearance that follows the system, with colour contrast
  checked automatically.
- Locks after 5 minutes without use, and clears copied passwords from the
  clipboard after 15 seconds.
- Stays in the system tray when the window is closed, with an icon that shows
  whether the vault is locked.
