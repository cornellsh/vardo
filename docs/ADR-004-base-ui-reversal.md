# ADR-004: React 19 + Base UI

**Status:** Accepted. **Supersedes `ADR-003`**, which chose Svelte 5.

This reverses a decision made a day earlier. That is uncomfortable, and the
reasoning for why it is nonetheless correct is at the bottom.

---

## The flip condition fired

`ADR-003` chose Svelte narrowly, and wrote down what would reverse it:

> **Bits UI stalls and no replacement appears.** Then hand-rolling accessible
> widgets becomes the job, and React Aria's existence outweighs everything
> above.

I framed that as something to watch for later. It is observable **now**, and I
had simply not looked hard enough at either side of it.

---

## What I did not know when writing ADR-003

### Base UI shipped, and it is not a minor entry

| | |
|---|---|
| v1.0 | December 2025; now 1.7.0 (August 2026) |
| Cadence | Monthly, unbroken: 1.1 Jan, 1.2 Feb, 1.3 Mar, 1.4 Apr, 1.5 May, 1.6 Jun, 1.7 Aug |
| Downloads | **~10.3M/week** |
| Components | 35, including Combobox, Autocomplete, Dialog, Menu, Popover |
| Authors | The creators of **Radix, Floating UI, and Material UI** |
| Backing | MUI — "a company with engineers, designers, and managers dedicated to the project" |

This is not a new competitor to Radix. It is the *same people*, having learned
from Radix, with a company paying for it and an explicit long-term maintenance
commitment.

The changelog is the tell. Entries like *"Add `toolbar` role to
`<Combobox.Chips>` to prevent NVDA from entering browse mode"* are the kind of
accessibility work that only accumulates when specialists are paid to do it for
years. I could not produce that. Nor could any one-person library.

### Radix is in worse shape than I assumed

Acquired by WorkOS; most original maintainers left. Bus factor 2, commits in
14 of the last 52 weeks. Its **co-creator Colm Tuite** called Radix "a
liability" and "the last option" he would consider for a serious project — and
Colm Tuite is now listed as a Base UI maintainer.

I had treated Radix as the React incumbent. It is the thing Base UI exists to
replace.

### Bits UI is one person

The Svelte answer to Radix, and my mitigation in `ADR-003`, is
[bits-ui](https://github.com/huntabyte/bits-ui): 1.03M weekly downloads, 3.5k
stars, actively maintained — and per its own documentation, *"built and is
maintained by Hunter Johnston"*, with design help and tooling help from two
others.

It is good software. It is also **bus factor 1, with a tenth of Base UI's
adoption**, and it is the load-bearing dependency underneath shadcn-svelte as
well.

---

## Why this changes the answer

`ADR-003`'s decisive argument was maintenance burden on a solo maintainer:
Svelte is ~30–40% less code, and Buttercup died of surface area rather than of
a bad framework.

That argument is still true, and it is now **outweighed by its own logic**.

**The code you do not write beats the code you write less of.** Base UI removes
more work than Svelte's terseness saves — a combobox, a listbox, a dialog, a
menu, focus management and popup positioning, all already correct.

**And the failure mode is asymmetric.** If Bits UI stalls, I do not inherit
"some components". I inherit *a headless accessibility library*: focus traps,
ARIA composition, screen-reader quirks across NVDA/JAWS/VoiceOver, RTL,
collection semantics. That is precisely the specialist code a solo maintainer
cannot keep correct — and it would land on me in exactly the situation where I
have least capacity to absorb it.

Choosing Svelte means betting a decade-long project on one person's continued
interest in a library I would be unable to take over. That is the *same bet*
whose failure I have been citing as the cautionary tale throughout this
project.

**The visual evidence points the same way.** Linear — the benchmark for this
class of interface — is React with Radix primitives, Emotion/StyleX, and a
hand-built store. Raycast's UI layer is React. This is correlation with React's
dominance more than causation, but it does establish that the stack reaches the
visual bar. Nothing establishes that for Svelte at this level of polish.

---

## What I lose, and what replaces it

**Svelte's compile-time `a11y_*` warnings.** A genuine loss, and the one thing
no other framework has.

Two things soften it, and I want to be honest that they soften rather than
replace:

1. `eslint-plugin-jsx-a11y` covers much of the same ground. It is opt-in rather
   than automatic, so it goes in CI at error level — same discipline `ADR-003`
   already committed to.
2. **The warnings mostly catch hand-rolled interactive elements** — a `div`
   used as a button, a missing label, a lost focus ring. Using Base UI means
   hand-rolling far fewer interactive elements in the first place, so the class
   of bug shrinks rather than being caught later.

**Less code.** React is more verbose, and that cost is real and permanent. It
is the price of the above.

**Bundle size.** ~45 KB versus ~5 KB of framework. For a desktop app loading
from local disk this is irrelevant, and I should not have leaned on it in
`RESEARCH.md` either.

---

## Decision

- **React 19** with the React Compiler.
- **Base UI** for every interactive primitive. Do not hand-roll a combobox,
  dialog, menu or popover.
- **Tailwind** plus a hand-built design system on top — Base UI is unstyled, so
  the look is entirely ours, which was always the point.
- **`eslint-plugin-jsx-a11y` at error level in CI**, plus real screen-reader
  testing before each release.
- **No `shadcn/ui`.** The copy-paste registry model means inheriting components
  into the repo and hand-merging upstream changes forever. Base UI directly,
  styled by us, is a smaller long-term surface.
- The frontend stays thin over the command surface in `UI-PLAN.md` §3, so this
  remains a replaceable layer.

---

## On reversing a decision this fast

Three framework decisions in two days, and this one contradicts yesterday's.
That pattern deserves scrutiny, so:

**Why this reversal is sound.** It is not a change of taste. `ADR-003` named a
falsifiable condition, and checking it properly — which is what the question
"what about Base UI?" prompted — showed it was already met. The correct
response to discovering a written flip condition has fired is to flip, not to
defend the prior decision.

**Why it should stop here.** The remaining uncertainty was concentrated in one
question: *is there a maintained, best-in-class primitives library, and on
which side?* That now has a clear answer, and it is unlikely to reverse — Base
UI has company backing, an eight-month unbroken release cadence, and the people
who invented the category.

**What would still move it.** MUI abandoning Base UI, or screen-reader testing
exposing something structural. Both are observable, neither is a matter of
preference.

**What has not changed at any point:** Tauri, the Rust core, and the rule that
secrets never enter the WebView in bulk. Every framework argument has been
about the top 10% of the stack, and the command surface in `UI-PLAN.md` §3 is
deliberately shaped so that this layer stays cheap to replace. That the layer
churned twice on paper and zero times in code is the design working, not
failing.

---

## Rejected, again

| Option | Why not |
|---|---|
| **Svelte 5 + Bits UI** | Superseded above. Excellent framework; the primitives dependency is bus factor 1 and unmaintainable by me if it stalls. |
| **React + Radix** | The incumbent, and its own co-creator calls it a liability. Base UI is the successor by the same authors. |
| **React + React Aria** | The other serious choice, and the deepest accessibility model available. Rejected on weight (~241 KB versus ~10 KB per component) and on an abstraction that is heavier than this app needs. Reconsider if Base UI ever falls short on a specific widget — they compose fine side by side. |
| **React + shadcn/ui** | Copy-paste registry means owning the components and hand-merging upstream forever. Sensible for shipping fast; wrong for a decade-long solo project. |
| **Ark UI** | Framework-agnostic via state machines, so it would survive a framework change. Smaller, and the a11y depth is not Base UI's. |
