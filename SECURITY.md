# Security policy

Vardo holds people's passwords, so security problems are taken seriously and
handled privately until they are fixed.

## Reporting a problem

**Please do not open a public issue.** A public report tells attackers about the
problem before anyone can fix it.

Report it privately instead, using GitHub's
[private vulnerability reporting](https://github.com/cornellsh/vardo/security/advisories/new).
Only the maintainer can see what you send.

If you cannot use GitHub, email **kornelshakh@proton.me** with "Vardo security"
in the subject.

It helps to include:

- what the problem is, and what an attacker could do with it;
- the steps to reproduce it, or a small example vault or file that triggers it;
- the version or commit you tested, and your operating system.

**Never send a real vault or a real master password.** If a vault is needed to
show the problem, create a new one with a throwaway password.

## What happens next

Vardo is maintained by one person, so these are honest targets rather than
guarantees:

- You will get a reply within **7 days** saying whether the report is
  understood and accepted.
- A fix for a serious problem is the top priority until it ships.
- You will be told before the fix is made public, and credited in the release
  notes if you would like to be.

## What counts

Anything that could expose, alter or destroy someone's passwords, for example:

- reading a vault's contents without the master password;
- a password or other secret reaching the window, the logs, a crash report, or
  a file outside the vault;
- a save, sync or merge that loses or corrupts data;
- a malformed `.kdbx` file that crashes Vardo or makes it misbehave;
- a way for another website or program to reach Vardo's Google Drive access.

Problems in software Vardo depends on are worth reporting too. If the fix
belongs upstream, the report will be passed on with your permission.

## Supported versions

Vardo has no releases yet. Security fixes go into the `main` branch.
