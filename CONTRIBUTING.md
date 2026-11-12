# Contributing to Vardo

Thank you for wanting to help. This page explains how to set up the project,
how to check your change, and what kind of changes fit.

If you are planning something larger than a small fix, please open an issue
first to talk it through. It is a shame when good work does not fit, and a short
conversation up front avoids that.

## Setting up

You need Rust, Node.js 20 or newer, and the system libraries listed under
[Installing](README.md#installing) in the README. Then:

```sh
git clone https://github.com/cornellsh/vardo.git
cd vardo/app
npm ci
npm run app          # opens Vardo with live reloading
```

To work on the interface without building the Rust side at all:

```sh
cd app
npm run build && npm run preview
```

That serves the interface against a pretend vault of about 400 logins, with
deliberately messy data. Its password is `demopass`. It cannot touch a real
file.

Optional, but recommended: install KeePassXC so that `keepassxc-cli` is on your
path. Several tests use it to confirm that other apps can open what Vardo
writes, and they skip themselves without it.

## Checking your change

Run this from the top of the repository before you send anything:

```sh
./check.sh
```

It checks formatting, runs the linters with warnings treated as errors, runs
every Rust and interface test, checks colour contrast, and builds the app. It
stops at the first failure. The same checks run automatically on every pull
request.

Useful smaller commands while you work:

| Command | What it does |
|---|---|
| `cargo test -p vardo-kdbx` | Tests one crate |
| `cd app && npm test` | Runs the interface tests |
| `cd app && npm run contrast` | Explains every colour-contrast measurement |
| `cargo fmt --all` | Formats the Rust code |

## What makes a change easy to accept

**It comes with a test.** For a bug fix, the best test is one that fails before
your change and passes after it.

**It keeps passwords out of the window.** Secrets stay in the Rust code. The
interface receives a password only when someone clicks Show. If a change needs a
secret in the interface, it needs a very good reason, and the reason belongs in
the pull request.

**It keeps files compatible.** Vardo must never change bytes in a vault that it
was not asked to change, and everything it writes must open in KeePassXC and
KeePassium. Changes to `vardo-kdbx` are held to this strictly.

**It explains why.** Commit messages and pull requests should say why the
change is needed and what was considered, not only what changed. The diff
already shows what changed.

**Its words are plain.** Anything a user reads should be short, specific, and
free of jargon. A message that fits on one line is better than a paragraph;
details can go behind a "Details" disclosure.

## What does not fit

Some ideas are reasonable on their own but go against what Vardo is for. To save
you the effort, these will not be accepted:

- **A Vardo server or account system.** Vardo has no servers and no accounts,
  and that is the point.
- **Analytics, telemetry or crash reporting that phones home.**
- **Third-party icon services.** Site icons are fetched from the site itself, so
  no other company learns which sites someone uses.
- **Merging individual fields of a login.** Combining the username from one
  device with the password from another can create a login that never existed.
  Vardo merges whole logins and keeps the losing version in history.
- **Silently upgrading someone's vault format.** The file keeps the version it
  arrived with.

## Where things are

- [README.md](README.md#for-developers) has a map of the code.
- [docs/STATUS.md](docs/STATUS.md) describes what works and how it was proven.
- [docs/ROADMAP.md](docs/ROADMAP.md) lists what is planned.
- [docs/DESIGN.md](docs/DESIGN.md) covers colours, type, motion and layout.
- The `ADR-*` files in [docs/](docs/) record each major decision and the
  reasoning behind it. If you want to reverse one, write a new ADR that argues
  against it, rather than editing the old one.

## Reporting bugs

Use the [issue tracker](https://github.com/cornellsh/vardo/issues). For anything
that could expose someone's passwords, follow [SECURITY.md](SECURITY.md)
instead.

Never attach a real vault or share a real master password, even a partial one.
If a vault is needed to show the problem, make a new one with a throwaway
password.

## License

Unless you say otherwise, anything you contribute is licensed under both the
Apache License 2.0 and the MIT License, like the rest of Vardo, without any
additional terms.

Everyone taking part is expected to follow the
[Code of Conduct](CODE_OF_CONDUCT.md).
