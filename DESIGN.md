# Design system

Base UI ships no styles, and Tailwind is a mechanism rather than a design. This
is the layer that decides what the app actually looks like.

---

## 1. The uncomfortable part first

You cannot install good design. What you can install is a **colour system**, a
**typeface**, an **icon set** and a **motion library** — and picking those well
removes most of the ways this goes wrong. The rest is a short list of
constraints applied without exception, and then iteration against real content.

There is also a trap worth naming before anything else.

### Do not build the Linear look

Near-black canvas, single purple accent, 13px rows, `⌘K` palette. It is
genuinely excellent design, and it has become **the default output of every AI
design tool** — because it is the statistical middle of recent SaaS. A December
2025 Hacker News thread asked why every B2B product now looks like Linear, and
the answer is that the floor for producing it by accident dropped to zero.

Which inverts what it signals. In 2020 a dark, dense interface read as *someone
sweated the details*. In 2026 it increasingly reads as *someone pressed enter in
a builder and shipped it*.

So: **steal the systems, not the look.** The surface ladder, hairlines instead
of shadows, one accent used scarcely — those are sound engineering. The
particular near-black-and-lavender outfit is now a costume everyone owns.

### What "Signal-like" means, precisely

Signal Desktop is not the Linear aesthetic. It is **light-first**, high
contrast, generously spaced, and almost chrome-free. It reads like an appliance
rather than a product launch, and it has looked broadly the same for a decade.

That is the right register for a vault. This app should look like something
that has been maintained for ten years, not like something that launched last
week — and it should look like *itself*, not like a project-management tool.

---

## 2. Colour — Radix Colors

The single highest-leverage thing to adopt, and it comes from the same people
as Base UI.

Twelve steps per scale, each step designed for a specific job, with a matched
dark scale and alpha variants for every one. Critically, **steps 11 and 12 are
guaranteed Lc 60 and Lc 90 APCA contrast on a step-2 background of the same
scale**. Contrast becomes a property of the system instead of something you
audit afterwards.

| Steps | Job |
|---|---|
| 1–2 | App and subtle component backgrounds |
| 3–5 | Component backgrounds: normal, hover, active |
| 6–8 | Borders: subtle, normal, focused |
| 9–10 | Solid backgrounds, and their hover |
| 11 | Low-contrast text |
| 12 | High-contrast text |

This matters here beyond aesthetics. Tailwind's palette is beautiful and
carries **no contrast guarantee whatsoever** — `bg-sky-500` tells you nothing
about whether the text on it is legible. A colour system with the guarantee
built in is worth more than one with more shades.

### The accent

Everything in this category is blue, purple, or near-black. 1Password is blue.
Bitwarden is blue. KeePassXC is blue-grey. Linear is lavender. Choosing another
blue means being invisible.

**Accent: `bronze`. Neutral: `sand`.**

Warm, quiet, and thematically exact without being literal — brass, keys, old
locks. It is unusual in software generally and unheard of in this category, and
because Radix ships it as a full scale with a dark counterpart, it costs nothing
to try. It is also one token: if it turns out wrong, changing it is a one-line
edit, which is the point of doing colour this way.

Semantic colours stay conventional, because this is not the place to be
interesting: `red` for destructive, `amber` for warnings, `jade` for success and
sync-healthy.

### Light first

Default to light, follow the system preference, and offer an explicit override.
Dark mode is designed properly rather than derived — but this app is not
dark-by-default, because that is the costume.

If dark: canvas at Radix `sand` step 1, **never `#000000`**. Pure black emits no
light, so elevated surfaces have nothing to contrast against and the whole thing
turns to mush. Elevation comes from stepping the surface, not from shadows.

---

## 3. Typography — Inter, plus a real monospace

**Interface: Inter Variable.** Designed for screens, tall x-height, enormous
character set, OFL. Linear uses it too — despite the widespread assumption that
their type is custom.

Three OpenType features are switched on for **functional** reasons, not taste:

| Feature | Why it is not decoration |
|---|---|
| `tnum` (tabular numbers) | TOTP codes and countdown timers must not shift width as digits change |
| `zero` (slashed zero) | Users read passwords character by character; `0` versus `O` has to be unambiguous |
| `calt` | Contextual alternates, general polish |

**Secrets, TOTP codes and UUIDs: Geist Mono.** Not a stylistic choice. When
someone reads a revealed password aloud or copies it by hand, `1`/`l`/`I` and
`0`/`O` must be distinguishable. A proportional font makes that genuinely
harder, which in a password manager is a usability bug rather than a
preference.

### Scale

Desktop density, tuned for scanning a list rather than reading prose.

```
display   24px / 1.2   weight 600   tracking -0.02em
title     17px / 1.3   weight 600   tracking -0.01em
body      14px / 1.5   weight 400
label     13px / 1.4   weight 500
meta      12px / 1.4   weight 400   step-11 colour
mono      13px / 1.5   Geist Mono
```

Six sizes. Hierarchy comes from **colour and spacing**, not from a dozen weights
— Raycast's rule of nothing being bold except actual headings is right.

---

## 4. Icons — Lucide

1,500+ icons, ISC, ~29M weekly downloads, and the best tree-shaking of the
field: **~106 bytes per icon** against Phosphor's ~694 B and react-icons' ~1,660 B.

The known objection is that Lucide is the shadcn default, so interfaces using it
"look subtly identical to hundreds of other products". Largely irrelevant here:
this app needs roughly **thirty icons**, all functional — copy, reveal, lock,
sync, search, warning. Nobody identifies a product by its copy icon. If a few
become signature (the lock, the sync state), draw those.

---

## 5. Motion — CSS first, Motion only if needed

Start with CSS transitions, `@starting-style` and view transitions. Add
[Motion](https://motion.dev) (formerly Framer Motion) only where CSS genuinely
cannot express it.

Motion is excellent — hybrid WAAPI engine, 120fps, used by Figma and Framer —
and its own maintainer has published *"Do you still need Framer Motion?"*
arguing CSS has closed much of the gap. For an app whose entire animation
budget is a few state transitions, CSS wins on dependency count.

Motion earns its place only if the merge-review diff or the quick-access overlay
needs shared-element transitions.

### Where animation is allowed

Exactly three places, all of which **explain a state change**:

1. **Unlock** — progress during the ~0.9 s Argon2 derivation. Not decoration:
   without it the app looks hung, which is precisely how KeePassXC feels.
2. **Quick-access overlay** — appearing and dismissing.
3. **Sync status** — transitioning between idle, working, merged, and needs-review.

Nothing on hover. Nothing on list rows. Nothing that runs more than once.

---

## 6. Tokens — Tailwind v4, CSS-first

Tailwind v4 moved configuration into CSS via `@theme`, and every token becomes
a real custom property. One source of truth for both utilities and bespoke CSS.

```css
@import "tailwindcss";
@import "@radix-ui/colors/sand.css";
@import "@radix-ui/colors/sand-dark.css";
@import "@radix-ui/colors/bronze.css";
@import "@radix-ui/colors/bronze-dark.css";

/* Two-stage on purpose. `@theme inline` bakes values in at build time, so
   toggling .dark at runtime would do nothing — the single most common way to
   break dark mode in v4. */
:root {
  --canvas:        var(--sand-1);
  --surface:       var(--sand-2);
  --surface-hover: var(--sand-3);
  --border:        var(--sand-6);
  --border-strong: var(--sand-7);
  --text:          var(--sand-12);
  --text-muted:    var(--sand-11);
  --accent:        var(--bronze-9);
  --accent-text:   var(--bronze-11);
}

@theme {
  --color-canvas: var(--canvas);
  --color-surface: var(--surface);
  --color-border: var(--border);
  --color-text: var(--text);
  --color-muted: var(--text-muted);
  --color-accent: var(--accent);

  --font-sans: "Inter Variable", system-ui, sans-serif;
  --font-mono: "Geist Mono Variable", ui-monospace, monospace;

  --radius-sm: 4px;
  --radius-md: 6px;
  --radius-lg: 10px;
}
```

Spacing stays on Tailwind's 4px base. Radii top out at 10px — heavily rounded
corners read as consumer software, and this is a tool.

---

## 7. The constraint list

Rules with no exceptions. Every one of them is a way the app gets worse if
broken once and then twice.

1. **One accent colour**, on no more than ~5% of any view. Raycast's discipline:
   when an accent is everywhere, it stops being signal.
2. **Elevation is a lighter surface, not a shadow.** Shadows are noisy and
   expensive; a two-percent lightness step reads as depth for free.
3. **Hairline borders**, 1px, from step 6. No thick rules, no dividers doing
   the job spacing should.
4. **Radius ≤ 10px.**
5. **Six type sizes.** Adding a seventh means the hierarchy is wrong.
6. **Focus rings are always visible** and never removed. Keyboard-first is a
   stated goal; a lost focus ring makes the app unusable, not just ugly.
7. **Nothing animates on hover.**
8. **Empty states say something useful.** No illustrations.
9. **Secrets render in mono, never proportional.**
10. **Every colour comes from a token.** No literal hex outside the token file.

---

## 8. How the design actually gets good

The tokens above prevent the app from being *bad*. Making it *good* is
different work, and it is not a shopping list.

**Study systems, not screenshots.** Raycast's four-step surface ladder and
zero-shadow rule; Linear's negative display tracking and 15px/1.6 body; Signal's
refusal to add chrome. Understand why each works, then discard the specifics.

**Design against real content.** A vault with 400 entries, some with 60-character
titles, some with none. Layouts that only work with tidy fake data are the
single most common way a design collapses on contact with users.

**The hard screens deserve real time.** Unlock, the merge review, and the
quick-access overlay are the three places this app is unlike anything else, and
the merge review in particular has no prior art — nobody else ships one.
Everything else is a list and a detail pane, which is solved.

**Iterate on the real thing.** Not in Figma. The app runs locally, the feedback
loop is milliseconds, and the tokens make sweeping changes a one-line edit.

---

## 9. Accessibility, since it decided the toolkit

Accessibility was the deciding argument against Flutter (`ADR-002`) and a live
concern in `ADR-003`. So it gets teeth here:

- **Radix Colors steps 11/12 carry an APCA guarantee**, which makes text
  contrast structural rather than audited.
- **`eslint-plugin-jsx-a11y` at error level in CI.** Non-negotiable, since
  React has no compile-time equivalent to what Svelte gives free.
- **Base UI for every interactive primitive.** Hand-rolling means owning ARIA
  composition and focus management forever.
- **A real screen reader before each release**, not just `axe`. Automated checks
  cannot see focus order or whether anything got announced.
- **Full keyboard reachability** is a release gate, not a feature.
