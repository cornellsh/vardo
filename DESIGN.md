# Design system — Telegram Desktop, ported

Not a design. A **port**. Every value below is lifted from
[`telegramdesktop/tdesktop`](https://github.com/telegramdesktop/tdesktop)
source — `colors.palette`, `day-blue.tdesktop-theme`,
`night-custom-base.tdesktop-theme`, `dialogs.style`, `window.style`,
`basic.style`.

Nothing here is invented. Where a value is missing from the source, the rule is
to go and read the source, not to make one up.

## Why this works for a vault

Telegram Desktop's layout is a left rail, a searchable list, and a detail pane.
A password manager is a group rail, a searchable entry list, and an entry pane.
The mapping is one-to-one, which is why this is a port and not an aesthetic
borrowed from an unrelated product.

| Telegram | Vardo |
|---|---|
| Folders side bar | Groups |
| Chat list (search at top) | Entry list |
| Chat view | Entry detail |
| Chat row: avatar, name, preview, time | Entry row: icon, title, username, modified |
| Right-click context menu | Copy / reveal / edit / delete |
| `Ctrl+F` search | Same |

---

## 1. Colour

Telegram ships a *day* and a *night* palette, and the constant names carry
meaning. Names are kept so any value can be traced back to source.

### Day — `day-blue.tdesktop-theme`

```
windowBg              #ffffff   app background
windowFg              #000000   body text
windowBgOver          #f1f1f1   hover background
windowBgRipple        #e5e5e5   pressed
windowSubTextFg       #999999   secondary text
windowBoldFg          #222222   titles
windowBgActive        #40a7e3   THE accent — selection, badges, primary buttons
windowFgActive        #ffffff   text on accent
windowActiveTextFg    #168acd   links, active text
activeButtonBgOver    #39a5db   primary button hover
activeButtonBgRipple  #2095d0   primary button pressed
lightButtonBgOver     #e3f1fa   secondary button hover
attentionButtonFg     #d14e4e   destructive
shadowFg              #00000018 hairline / shadow
dialogsBgOver         #f1f1f1   list row hover
dialogsBgActive       #419fd9   list row selected
dialogsNameFg         #222222   row title
dialogsTextFg         #999999   row subtitle
dialogsUnreadBg       #40a7e3   badge
```

### Night — `night-custom-base.tdesktop-theme`

```
windowBg              #17212b   app background
windowFg              #f5f5f5   body text
windowBgOver          #232e3c   hover background
windowSubTextFg       #708499   secondary text
windowBoldFg          #e9e8e8   titles
windowBgActive        #5288c1   accent
windowFgActive        #ffffff   text on accent
windowActiveTextFg    #6ab3f3   links, active text
activeButtonBg        #2f6ea5   primary button
attentionButtonFg     #ec3942   destructive
shadowFg              #04080e56 hairline / shadow
titleBg               #1f2936   inactive window title bar
sideBarBg             #0e1621   folders rail — darker than the list
dialogsBg             #17212b   list background
dialogsBgOver         #202b36   row hover
dialogsBgActive       #2b5278   row selected
dialogsNameFg         #f5f5f5   row title
dialogsTextFg         #7f91a4   row subtitle
dialogsUnreadBg       #4082bc   badge
searchedBarBg         #202b38   results bar
filterInputActiveBg   #242f3d   focused search field
```

Note `sideBarBg #0e1621` is **darker** than `windowBg #17212b`. Telegram's
depth model runs darkest-rail → list → content, not the elevation ladder most
dark themes use. Keep it.

### Vault-specific additions

Only where Telegram genuinely has no counterpart. Both taken from existing
palette entries rather than invented:

| Need | Day | Night | Source |
|---|---|---|---|
| Destructive | `#d14e4e` | `#ec3942` | `attentionButtonFg` |
| Sync healthy | `#40a7e3` | `#5288c1` | `windowBgActive` — not a new green |
| Warning / needs review | `#d14e4e` | `#ec3942` | `attentionButtonFg` |

Telegram has essentially two states, normal and attention. Resisting a
five-colour semantic system is part of copying it properly.

---

## 2. Typography

From `basic.style` and `style.txt`:

```
defaultFontFamily   'Open Sans'
semibold            'Open Sans Semibold'
fsize               13px     ← base UI size
boxFontSize         14px     ← dialogs/modals
dialogsDateFont     13px
msgDateFont         13px
```

**13px base.** Denser than most web apps, and it is the single biggest reason
Telegram feels like a desktop application rather than a website. Do not
"improve" it to 14 or 15.

Weights are **regular and semibold only**. Telegram has no bold body text and
no light weight. Hierarchy comes from colour — `windowBoldFg` for titles,
`windowSubTextFg` for secondary — not from weight.

### Fonts to actually ship

Open Sans is what tdesktop bundles, and it falls back to Segoe UI on Windows
when loading fails. Ship Open Sans Variable to match, with the platform stack
behind it:

```css
--font-ui: "Open Sans", "Segoe UI", -apple-system, system-ui, sans-serif;
```

One deviation, and it is functional rather than stylistic: **secrets, TOTP
codes and UUIDs render in a monospace with a slashed zero.** Telegram has no
equivalent because Telegram never asks you to read a random string character by
character. A password manager does, and `0`/`O`, `1`/`l`/`I` have to be
distinguishable. Use `ui-monospace`, and enable `tnum` on TOTP so the countdown
does not shift width.

---

## 3. Metrics

From `window.style`, `dialogs.style`:

```
windowMinWidth            380px
windowMinHeight           480px
windowDefaultWidth        800px
windowDefaultHeight       600px
windowBigDefaultWidth    1024px
windowBigDefaultHeight    768px

columnMinimalWidthLeft    260px    groups rail min
columnMaximalWidthLeft    540px    groups rail max
columnMinimalWidthMain    380px    entry list min
columnMinimalWidthThird   292px    detail pane min
columnMaximalWidthThird   392px    detail pane max

dialogsRowHeight           62px    ← list row
dialogsPadding      10px, 8px
dialogsFilterPadding 7px, 7px      search field
dialogsUnreadHeight        19px    badge
```

Two behaviours worth copying exactly:

**List width is proportional, then clamped.** From `mainwidget.h`:

```
chatsListWidth = clamp(windowWidth * 5 / 14, min, max)
```

Not a fixed pixel width and not a naive percentage.

**Three columns collapse to two, then to one**, based on whether the body width
clears `columnMinimalWidthLeft + columnMinimalWidthMain` and then
`+ columnMinimalWidthThird`. Same thresholds.

`dialogsRowHeight: 62px` is generous because Telegram rows carry an avatar plus
two text lines. An entry row carries an icon plus title and username — the same
shape, so keep 62px rather than compressing.

---

## 4. Rules taken from the source

1. **One accent**: `windowBgActive`. Selection, badges, primary buttons, focus.
   Nothing else is coloured.
2. **Two weights**: regular and semibold.
3. **13px base**, 14px in dialogs.
4. **Hierarchy by colour**, not by weight or size: `windowBoldFg` → `windowFg`
   → `windowSubTextFg`.
5. **Hairlines and flat fills**, not shadows. `shadowFg` is `#00000018` in day
   — 9% black — and used for separators, not elevation.
6. **Selected rows get a solid accent fill** (`dialogsBgActive`), not a tint or
   a left border.
7. **Small radii.** Telegram is nearly square: buttons and inputs are subtly
   rounded, avatars are circles, nothing else is.
8. **The rail is darker than the list, which is darker than nothing.** Depth
   runs one direction.

---

## 5. Where the port must be extended

Three screens have no Telegram counterpart. Build them in the same language —
same palette, same 13px, same row height — rather than inventing a style:

**Unlock.** Closest analogue is Telegram's passcode lock. Centred, minimal,
plus a progress indicator for the ~0.9 s Argon2 derivation.

**Merge review.** No counterpart anywhere. Build it as a Telegram *box*
(`boxFontSize: 14px`, `boxBg: windowBg`) containing two lists: changes to apply,
and items needing a decision. Reuse the row shape.

**Quick access overlay.** Closest analogue is Telegram's search. Same
`filterInputActiveBg`, same result-row shape, floating rather than docked.

---

## 6. Implementation

Palette as CSS custom properties, keeping Telegram's names so any value traces
to source. Tailwind v4 `@theme` maps them to utilities.

```css
:root {
  --window-bg:            #ffffff;
  --window-fg:            #000000;
  --window-bg-over:       #f1f1f1;
  --window-bg-ripple:     #e5e5e5;
  --window-sub-text-fg:   #999999;
  --window-bold-fg:       #222222;
  --window-bg-active:     #40a7e3;
  --window-fg-active:     #ffffff;
  --window-active-text-fg:#168acd;
  --active-button-bg-over:#39a5db;
  --light-button-bg-over: #e3f1fa;
  --attention-button-fg:  #d14e4e;
  --shadow-fg:            #00000018;
  --dialogs-bg-over:      #f1f1f1;
  --dialogs-bg-active:    #419fd9;
  --side-bar-bg:          #f1f1f1;
}

.dark {
  --window-bg:            #17212b;
  --window-fg:            #f5f5f5;
  --window-bg-over:       #232e3c;
  --window-sub-text-fg:   #708499;
  --window-bold-fg:       #e9e8e8;
  --window-bg-active:     #5288c1;
  --window-fg-active:     #ffffff;
  --window-active-text-fg:#6ab3f3;
  --active-button-bg-over:#2f6ea5;
  --attention-button-fg:  #ec3942;
  --shadow-fg:            #04080e56;
  --dialogs-bg-over:      #202b36;
  --dialogs-bg-active:    #2b5278;
  --side-bar-bg:          #0e1621;
  --filter-input-active-bg:#242f3d;
}

/* Not `@theme inline` — that bakes values at build time and dark mode
   silently stops working. */
@theme {
  --color-window: var(--window-bg);
  --color-fg: var(--window-fg);
  --color-muted: var(--window-sub-text-fg);
  --color-bold: var(--window-bold-fg);
  --color-accent: var(--window-bg-active);
  --color-attention: var(--attention-button-fg);
  --color-rail: var(--side-bar-bg);

  --font-ui: "Open Sans", "Segoe UI", -apple-system, system-ui, sans-serif;
  --text-base: 13px;
  --text-box: 14px;
  --spacing-row: 62px;
}
```

Base UI supplies behaviour; these tokens supply appearance. Base UI ships no
CSS, which is exactly why a complete external palette is usable.

---

## 7. Accessibility

Telegram's palette is not APCA-verified, so contrast has to be checked rather
than assumed.

- **Verify every text-on-background pair** at build time. Two are already
  suspect: day `windowSubTextFg #999999` on `#ffffff` is ~2.8:1, below WCAG AA
  for body text, and `dialogsBgActive #419fd9` with white text is ~2.9:1.
- Where a pair fails, **darken the foreground and record the deviation here**
  with the measured ratio. Do not silently keep a failing value because
  Telegram ships it, and do not silently re-theme the app either.
- `eslint-plugin-jsx-a11y` at error level in CI.
- Base UI for every interactive primitive.
- Real screen-reader pass before each release.
