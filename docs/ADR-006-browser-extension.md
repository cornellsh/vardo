# ADR-006: A Vardo browser extension

**Status:** accepted. Reverses the scope line in `RESEARCH.md` §9 and
`CRITIQUE.md` §3.5, which said never to ship one.

## Why reverse it

Without autofill, using Vardo on the web means switching windows, copying,
switching back and pasting, dozens of times a day. People who live with that
start reusing passwords to make it bearable, which defeats the point of a
password manager. Autofill is not a convenience feature here; it is what makes
the rest of Vardo usable every day.

The earlier documents agreed autofill mattered. What they rejected was building
an extension, in favour of speaking KeePassXC-Browser's protocol so that its
existing extension would talk to Vardo unchanged. That alternative was
considered properly, and it is still the cheaper one. It loses on what it would
ask users to accept.

## The alternative: reuse KeePassXC-Browser

**For it.** It is mature, maintained, already in every browser's store, and
handles the hard parts of autofill well: multi-step logins, iframes, unusual
forms. Its protocol is documented. Vardo would ship a native host and nothing
in the browser.

**Against it.** Taken from its own manifest (version 1.10.4.1):

- It runs its scripts in **every frame of every page you visit**
  (`<all_urls>`, `all_frames: true`), and asks for `tabs`, `cookies`,
  `webRequest` and `webNavigation`. Chrome describes that to the user as
  *"Read and change all your data on all websites."*
- It injects its own clickable fill menu into pages. In 2025 Marek Tóth showed
  at DEF CON 33 that menus injected this way can be made invisible by the page
  and clicked through a fake cookie banner, filling credentials into a hidden
  form. All eleven password managers tested were vulnerable in their default
  configuration, and KeePassXC-Browser up to 1.9.9.2 was shown to be as well.
- Its native host is registered under KeePassXC's own name, so Vardo and
  KeePassXC installed side by side would fight over it.
- Its protocol adds its own encryption and pairing layer on top of native
  messaging, which Vardo would have to reimplement and then follow as it
  evolves.

The first two are the deciding points. A password manager asking for access to
every page, in order to inject UI that pages can attack, is the opposite of the
trade Vardo makes everywhere else.

## Decision

Vardo ships its own Chromium extension, designed around one rule: **the
extension sees nothing until the user asks it to fill.**

### 1. No access to pages until the user acts

The extension requests `activeTab`, `scripting`, `nativeMessaging` and
`storage`, and **no host permissions**. Chrome grants access to the current tab
only when the user clicks the toolbar button or presses the shortcut, and only
until they navigate away. Chrome therefore shows no "read and change all your
data" warning at install, because it is not true.

Consequences, accepted deliberately:

- The extension does not know which sites you visit. It cannot show a badge with
  the number of logins for a page before you click.
- There is no automatic filling when a page loads, ever. Filling always follows
  a click or a key press.

This is the configuration Tóth recommends users set by hand ("site access: on
click"). Vardo's extension is built that way, so nobody has to find the setting.

### 2. No UI injected into pages

Choosing a login happens in the extension's popup, which belongs to the browser
and cannot be covered, hidden or clicked through by the page. Nothing Vardo
draws ever lives in a page's DOM, so the clickjacking technique above has
nothing to target.

The cost is that there is no little Vardo icon inside password fields. The
keyboard shortcut covers the case that icon exists for.

### 3. Matching happens in Rust, against the URL the browser reports

The extension takes the page address from Chrome's tab information, never from
anything the page itself says. Vardo compares it with each login's saved address
in `vardo-core`, where it can be tested exhaustively:

- **Exact host** matches. These are what the shortcut fills.
- **Subdomain** matches, where the page is on a subdomain of the saved site.
  These are listed in the popup and need an explicit choice. Tóth found that ten
  of eleven managers filled these automatically, which turns cross-site
  scripting on any subdomain into theft of the main site's login.
- **Never** across a downgrade from `https` to `http`, never on a different
  port, and never by matching a public suffix such as `github.io` as though it
  were one site.

When the extension asks to fill a login, Vardo checks the match again before
returning anything. The extension choosing a login does not make it a match.

### 4. Secrets enter the browser only to be typed into the page

| Action | Where the secret goes |
|---|---|
| Fill | Vardo, to the extension, into the page's fields. Nowhere else. |
| Copy password | Vardo copies it itself and clears the clipboard after 15 seconds. The extension never sees it. |
| List logins | Titles, usernames and match quality only. No passwords. |
| Unlock | In Vardo's own window. The master password is never typed into the browser. |

Filling targets the top frame only. Logins inside cross-origin iframes are not
filled in the first version; they are a common way to attack autofill, and they
deserve their own design.

### 5. Transport: native messaging to the running app

```
Extension ──native messaging──▶ vardo-browser-host ──local socket──▶ Vardo
           (Chrome launches it,     (a small relay,       (the vault stays here,
            for our ID only)         no vault, no keys)    in this process only)
```

- **Native messaging,** not a local web server. Chrome starts the host only for
  extension IDs listed in its manifest. A localhost port can be reached by any
  website, and would need its own defence against that.
- **The host is a relay.** It holds no vault and no keys, and exits when the
  browser is done with it. The vault never leaves the Vardo process.
- **The socket** lives in the user's private runtime directory, readable only by
  that user.

Other programs running as the same user can reach that socket. They could
equally read Vardo's memory or log keystrokes, so a pairing secret stored on the
same disk would add ceremony without protection. Every desktop password manager
shares this boundary; Vardo states it rather than implying otherwise.

## What the first version does

- Fill the matching login on the current site, from the popup or with
  <kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd>.

  Not <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd>, which was the first
  choice and is what Bitwarden uses. Chrome took it in 2026 to collapse
  vertical tabs, and a browser never gives an extension a shortcut it owns.
  It refused it silently: the end-to-end check found the command unassigned.
  Because any browser can do that, the popup notices when the shortcut is
  missing and offers to set one.
- Fill the two-factor code on the page that asks for it, with the same
  shortcut.
- Search the whole vault from the popup, and copy a username or password
  through Vardo.
- Say when Vardo is locked or not running, and bring its window forward to
  unlock.

## What comes later

- Offering to save a login after you sign up or change a password.
- Generating a password into a sign-up form.
- Logins inside iframes, with an origin check per frame.
- Firefox, which supports the same native messaging with a different manifest.
- Windows, where the socket becomes a named pipe.
- Publishing to the Chrome Web Store.

## Sources

- KeePassXC-Browser manifest:
  <https://github.com/keepassxreboot/keepassxc-browser/blob/develop/keepassxc-browser/manifest.json>
- Marek Tóth, *DOM-based Extension Clickjacking: Your Password Manager Data at
  Risk*, DEF CON 33, August 2025:
  <https://marektoth.com/blog/dom-based-extension-clickjacking/>
- CERT/CC VU#516608, *Multiple Password Managers Vulnerable to Clickjacking
  Attacks*: <https://www.kb.cert.org/vuls/id/516608>
- Chrome native messaging:
  <https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging>
- Chrome `activeTab`:
  <https://developer.chrome.com/docs/extensions/develop/concepts/activeTab>
