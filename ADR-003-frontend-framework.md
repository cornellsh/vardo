# ADR-003: Frontend framework

**Status:** Accepted — Svelte 5. But **both reasons I originally gave were
wrong**, and the real decision is much closer than `RESEARCH.md` implied.

---

## What I said, and why it does not hold

`RESEARCH.md` §4 picked Svelte in one line: *"smaller runtime = less to audit;
no virtual DOM retaining stale copies of decrypted strings."*

### "Smaller runtime = less to audit" — not a real argument

Nobody audits React's runtime. Nobody audits Svelte's either. The threat model
here is not "the framework is malicious", and 45 KB versus 5 KB is not an audit
surface anyone actually reads. This was a bundle-size argument wearing a
security costume.

### "No virtual DOM retaining stale copies of decrypted strings" — confidently wrong

This one I should have checked, because it is not merely weak, it is false.

JavaScript strings are **immutable**. Assigning over a variable allocates a new
string and leaves the old one on the heap. There is **no way to force garbage
collection** and no way to zeroize. Node has an open issue (#59965) asking for
zeroizable, pinnable memory precisely because none exists, and a WebWeWant
request for `SecureString` was met with the observation that if an attacker has
an arbitrary read primitive in the content process, they can read anything
anyway.

None of that varies by framework. React's fiber tree and Svelte's compiled DOM
operations hold secrets **exactly as badly as each other**. I invented a
distinction that does not exist.

### What actually follows from that

The finding is worth keeping even though it demolishes my argument, because it
**strengthens the rule this project already has**: since no JavaScript
framework can zeroize a secret, the only defence is not to put secrets in the
WebView at all.

That is already the design (`UI-PLAN.md` §3): `search()` and `entry()` cannot
return a protected value, `reveal()` hands over one field at a time, and
`copy()` writes the clipboard from Rust. The framework is *irrelevant* to this
concern, which is the honest conclusion.

---

## The real trade

With both original reasons gone, this comes down to two things that genuinely
differ.

### For Svelte: a compile-time accessibility floor

Svelte's compiler emits `a11y_*` warnings for inaccessible markup — a `<div>`
with a click handler and no keyboard handler, a missing label, a missing
`alt`, `autofocus`, `accesskey`, `aria-*` on elements that cannot take it.
**No other mainstream framework does this out of the box.**

That matters here for a specific reason: I rejected Flutter in `ADR-002`
partly on accessibility, citing dictation and screen-reader tools that cannot
find its text fields. Having made accessibility a deciding factor once, I do
not get to ignore it now.

### For React: React Aria, and it is the strongest argument against Svelte

Geoff Rich — a Svelte maintainer — puts the counter better than I could:

> The Curse of React is that the average React app is less accessible than the
> average jQuery app… Since Svelte is still a developing ecosystem, it suffers
> even more from the Curse of React… the best thinkers in accessibility are
> focusing on that ecosystem (React ARIA, Downshift, and others).

So the split is: **Svelte gives you a linter, React gives you solved widgets.**
A compile-time warning tells you the combobox is wrong. React Aria gives you a
combobox that is right — years of focus management, screen-reader quirks and
WAI-ARIA authoring practice, already debugged.

For a hand-rolled design system that needs a combobox (search), a listbox
(results), a dialog (merge review) and correct focus handling across lock and
unlock, that is worth more than lint warnings.

---

## Why Svelte still wins, narrowly

**The widget set is small.** Four genuinely hard widgets: combobox, listbox,
dialog, and focus management. Not forty. Svelte has real headless primitives
for exactly these — Bits UI, from the shadcn-svelte and Melt UI people — which
is the Radix-equivalent layer. Younger than React Aria, but this is a tractable
surface, not an enterprise component library.

**The decisive factor is the same one that decided the toolkit: maintenance
burden on one person.** Svelte is consistently ~30–40% less code for the same
component. Buttercup did not die of a bad framework choice, it died because one
maintainer could not carry the surface area. Anything that shrinks the volume
of code a single person has to keep alive for a decade is worth more here than
it would be on a team.

**The compile-time floor catches what actually goes wrong.** Most real
accessibility bugs are not subtle ARIA composition — they are a `div` used as a
button, a missing label, a lost focus outline. Svelte catches those on every
build, automatically, forever. React catches them only if `eslint-plugin-jsx-a11y`
is installed, configured, and not being ignored.

**Signals are where the field landed.** Svelte 5, Solid, Vue, Angular and Preact
all converged on fine-grained reactivity, and TC39 is standardising it. React
deliberately sat it out. Not decisive, but it means Svelte's model is the
direction of travel rather than an idiosyncrasy.

---

## Commitments this decision carries

Choosing the smaller ecosystem means accepting obligations, not hand-waving
them:

1. **Use Bits UI for the four hard widgets.** Do not hand-roll a combobox. If
   Bits UI is unmaintained when the time comes, vendor it — it is small — and
   port React Aria's behaviour rather than improvising.
2. **`a11y` warnings are errors in CI.** A compile-time floor is worthless if
   it is routinely silenced. `svelte-ignore` requires a comment explaining why.
3. **Test with a real screen reader** before each release, not just axe. Every
   framework's automated checks miss dynamic announcement and focus-order bugs.
4. **Keep the frontend thin.** The command surface in `UI-PLAN.md` §3 is the
   real API; the UI is a rendering of it. If the framework has to be replaced
   in three years, that should be a rewrite of `ui/`, not of the app.

---

## What would flip this

- **Bits UI stalls and no replacement appears.** Then hand-rolling accessible
  widgets becomes the job, and React Aria's existence outweighs everything
  above.
- **Screen-reader testing finds problems Svelte's model makes hard to fix.**
  Accessibility was a deciding factor against Flutter; it has to be allowed to
  decide against Svelte too.
- **This stops being a solo project.** The code-volume argument is worth much
  less with three contributors, and React's pool is ~40× larger.

---

## Rejected

| Option | Why not |
|---|---|
| **React 19** | The serious alternative, and React Aria is a genuine advantage. Loses on code volume for a solo maintainer and on the automatic a11y floor. Revisit if either flip condition above fires. |
| **Solid** | Fastest, and JSX suits React refugees, but a smaller ecosystem than Svelte *and* no compile-time a11y checks. Strictly worse here than either candidate. |
| **Vue 3** | Larger ecosystem than Svelte, mature, signals-based. But no compile-time a11y and no React Aria equivalent — it loses to Svelte on the first count and to React on the second. |
| **No framework** | The UI is a list, a detail pane, a search field and a few dialogs, so it is tempting. It ends with a worse framework written by one person, and reactivity bugs in a password manager are not a good place to learn that lesson. |
| **Lit / web components** | Best longevity story on paper. Poor DX, small ecosystem, and the a11y primitives are not there either. |

---

## Correction to `ADR-002`

`ADR-002` opens by saying its decision is coupled to "ADR-003 (no mobile app)".
No such ADR exists — the mobile decision lives in `CRITIQUE.md` §3.5. This
document is a different ADR-003, and that reference has been fixed.
