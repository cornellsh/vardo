# Documentation

For using Vardo, start with the [README](../README.md) at the top of the
project. Everything here is for people working on Vardo, or curious about how
and why it was built the way it was.

## Using Vardo

- [google-drive-setup.md](google-drive-setup.md) explains how to set up Google
  Drive in a copy you built yourself.

## How it works today

- [STATUS.md](STATUS.md) covers what works, and how each part was proven.
- [ROADMAP.md](ROADMAP.md) lists what is wrong or missing, in the order it will
  be fixed.
- [DESIGN.md](DESIGN.md) is the visual language: colour, type, icons, motion,
  and why each deviates from its Telegram Desktop source.
- [UI-PLAN.md](UI-PLAN.md) covers the screens and the order they were built.

## Decisions

Each decision record states one choice, the alternatives, and the evidence.
Later records can overturn earlier ones; when that happens the new record
argues against the old, and the old one stays as it was.

| Record | Decision |
|---|---|
| [ADR-001](ADR-001-sync.md) | Sync talks to Google Drive directly, rather than through a synced folder |
| [ADR-002](ADR-002-ui-toolkit.md) | Tauri rather than Flutter for the desktop app |
| [ADR-003](ADR-003-frontend-framework.md) | Svelte or React, with two wrong reasons retracted (superseded by ADR-004) |
| [ADR-004](ADR-004-base-ui-reversal.md) | React 19 with Base UI |
| [ADR-005](ADR-005-backend-strategy.md) | Google Drive is one storage backend, not the architecture |
| [ADR-006](ADR-006-browser-extension.md) | A Vardo browser extension, reversing the earlier decision never to ship one |

## History

Written before most of the code, and kept because the decisions above argue
against them. They describe intentions, not the current state.

- [RESEARCH.md](RESEARCH.md) is the original landscape survey and architecture.
- [CRITIQUE.md](CRITIQUE.md) is a red team of that research, with corrections.
- [PHASE0-FINDINGS.md](PHASE0-FINDINGS.md) explains why the KeePass file layer
  was written rather than borrowed.
- [../fuzz/README.md](../fuzz/README.md) records what fuzzing found.
