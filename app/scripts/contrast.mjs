#!/usr/bin/env node
/**
 * Contrast gate for the ported Telegram palette.
 *
 * DESIGN.md §7 says Telegram's palette is not APCA-verified, so contrast has to
 * be checked rather than assumed. This makes that a build step instead of a
 * claim in a comment: it reads the real token values out of tokens.css,
 * resolves var() chains, and fails if any declared pair drops below its
 * threshold.
 *
 * Usage:  node scripts/contrast.mjs [--verbose]
 */

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const TOKENS = join(HERE, '..', 'src', 'styles', 'tokens.css');

/** WCAG 2.1 minimums. 13px is "normal text" — large-text 3:1 does not apply. */
const AA_NORMAL = 4.5;
const AA_LARGE = 3.0;
/** Non-text UI components (borders, focus rings, indicators) — WCAG 1.4.11. */
const AA_NONTEXT = 3.0;

// --- colour maths -----------------------------------------------------------

function parseHex(hex) {
  const h = hex.replace('#', '').trim();
  const full = h.length === 3 ? [...h].map((c) => c + c).join('') : h;
  // Trailing alpha (Telegram writes #00000018) is ignored: these are used as
  // hairlines over unknown backgrounds and are not text.
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16));
}

function channel(c) {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance(hex) {
  const [r, g, b] = parseHex(hex);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(a, b) {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

// --- token extraction -------------------------------------------------------

/** Pull `--name: value;` pairs out of a single CSS block. */
function readBlock(css, selector) {
  const start = css.indexOf(selector);
  if (start === -1) throw new Error(`no ${selector} block in tokens.css`);
  const open = css.indexOf('{', start);
  const close = css.indexOf('\n}', open);
  const body = css.slice(open + 1, close);
  const out = {};
  for (const line of body.split('\n')) {
    const m = /^\s*(--[a-z0-9-]+)\s*:\s*([^;]+);/i.exec(line);
    if (m) out[m[1]] = m[2].trim();
  }
  return out;
}

/** Resolve `var(--x)` chains down to a literal hex. */
function resolve(vars, name, seen = new Set()) {
  const value = vars[name];
  if (value === undefined) throw new Error(`undefined token: ${name}`);
  const m = /^var\((--[a-z0-9-]+)\)$/i.exec(value);
  if (m) {
    if (seen.has(name)) throw new Error(`cyclic token: ${name}`);
    seen.add(name);
    return resolve(vars, m[1], seen);
  }
  if (!value.startsWith('#')) throw new Error(`token ${name} is not a colour: ${value}`);
  return value;
}

// --- the pairs that must hold ----------------------------------------------

/**
 * Every foreground/background combination the UI actually renders. Adding a new
 * coloured text combination to a component means adding it here too — that is
 * the point of the gate.
 */
const PAIRS = [
  // Entry list — the densest text in the app.
  ['dialogs-name-fg', 'dialogs-bg', AA_NORMAL, 'entry title'],
  ['dialogs-text-fg', 'dialogs-bg', AA_NORMAL, 'entry subtitle'],
  ['dialogs-name-fg', 'dialogs-bg-over', AA_NORMAL, 'entry title, hovered'],
  ['dialogs-text-fg', 'dialogs-bg-over', AA_NORMAL, 'entry subtitle, hovered'],
  ['window-fg-active', 'dialogs-bg-active', AA_NORMAL, 'entry title, selected'],

  // Chrome.
  ['window-fg', 'window-bg', AA_NORMAL, 'body text'],
  ['window-bold-fg', 'window-bg', AA_NORMAL, 'headings'],
  ['window-sub-text-fg', 'window-bg', AA_NORMAL, 'secondary text'],
  ['window-sub-text-fg', 'window-bg-over', AA_NORMAL, 'secondary text, hovered'],
  ['window-sub-text-fg', 'side-bar-bg', AA_NORMAL, 'group counts in the rail'],
  ['window-bold-fg', 'side-bar-bg', AA_NORMAL, 'group names in the rail'],

  // Actions.
  ['window-active-text-fg', 'window-bg', AA_NORMAL, 'links'],
  ['window-fg-active', 'active-button-bg', AA_NORMAL, 'primary button'],
  ['window-fg-active', 'active-button-bg-over', AA_NORMAL, 'primary button, hovered'],
  ['attention-button-fg', 'window-bg', AA_NORMAL, 'destructive'],
  ['attention-button-fg', 'box-bg', AA_NORMAL, 'destructive in a dialog'],
  ['window-fg-active', 'dialogs-unread-bg', AA_NORMAL, 'badge'],

  // Non-text: 3:1 under WCAG 1.4.11.
  ['window-bg-active', 'window-bg', AA_NONTEXT, 'focus ring'],
  ['window-bg-active', 'side-bar-bg', AA_NONTEXT, 'focus ring in the rail'],
];

// --- run --------------------------------------------------------------------

const verbose = process.argv.includes('--verbose');
const css = readFileSync(TOKENS, 'utf8');

const day = readBlock(css, ':root');
const nightOverrides = readBlock(css, '.dark');
const night = { ...day, ...nightOverrides };

let failures = 0;
let checked = 0;

for (const [themeName, vars] of [
  ['day', day],
  ['night', night],
]) {
  for (const [fg, bg, min, what] of PAIRS) {
    const fgHex = resolve(vars, `--${fg}`);
    const bgHex = resolve(vars, `--${bg}`);
    const ratio = contrast(fgHex, bgHex);
    checked++;
    const ok = ratio >= min;
    if (!ok) failures++;
    if (!ok || verbose) {
      const tag = ok ? 'ok  ' : 'FAIL';
      console.log(
        `${tag} ${themeName.padEnd(5)} ${what.padEnd(28)} ` +
          `${fgHex} on ${bgHex}  ${ratio.toFixed(2)}:1  (need ${min.toFixed(1)})`,
      );
    }
  }
}

if (failures > 0) {
  console.error(
    `\ncontrast: ${failures} of ${checked} pairs below threshold.\n` +
      `Fix the token, or if the deviation is deliberate, change the threshold ` +
      `here and record why in tokens.css and DESIGN.md.`,
  );
  process.exit(1);
}

console.log(`contrast: ${checked} pairs pass (AA ${AA_NORMAL}:1 text, ${AA_NONTEXT}:1 non-text)`);
void AA_LARGE;
