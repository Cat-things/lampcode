# LampCode website: design tokens and rules

Implement strictly from this file. If a component needs a value that isn't here, add it here first.

## Concept: the beam

A lamp on a desk in a dark room lights exactly what you point it at and nothing else. That is LampCode's privacy model: your prompt travels as one narrow beam to the one provider you picked, and the rest of the room stays dark. The whole site is built on that image:

- The page is the dark room: a deep ink-blue night, not neutral black.
- The accent is cool lamplight, used only where something is "lit": the primary action, the active state, the path data actually takes.
- The signature section, **"what leaves your computer"**, makes the metaphor literal: a single beam from your machine to one provider; each toggle (web search, updates, MCP) adds another thin, labelled beam. Everything that never leaves stays unlit.
- The hero foreshadows it: a soft cone of the same light falls from the top left onto the app screenshot.

Boldness is spent in one place: the beam. Everything else stays quiet, aligned and disciplined.

## Color

All text pairs below meet WCAG AA (4.5:1) for body text; ratios were computed, not estimated.

### Dark (default)

| Token | Hex | Role |
|---|---|---|
| `--night` | `#0D1017` | page background |
| `--surface-1` | `#131722` | sections, nav (at 72% opacity + blur) |
| `--surface-2` | `#1A1F2C` | cards |
| `--surface-3` | `#232A3A` | raised: popovers, active tab, window chrome |
| `--line` | `rgba(233,237,244,0.08)` | hairline borders |
| `--line-strong` | `rgba(233,237,244,0.16)` | input borders, dividers that carry meaning |
| `--ink` | `#E9EDF4` | primary text |
| `--ink-muted` | `#A0A9B8` | body copy, descriptions |
| `--ink-faint` | `#8A94A6` | captions, metadata |
| `--beam` | `#8FD3FF` | the one accent |
| `--beam-ink` | `#0D1017` | text on beam-filled buttons |
| `--beam-glow` | `rgba(143,211,255,0.14)` | radial light behind hero / active beams |

| Text on → | night | surface-1 | surface-2 | surface-3 |
|---|---|---|---|---|
| ink | 16.21 | 15.24 | 14.01 | 12.22 |
| ink-muted | 8.03 | 7.55 | 6.94 | 6.05 |
| ink-faint | 6.22 | 5.85 | 5.38 | 4.69 |
| beam | 11.72 | 11.02 | 10.13 | 8.83 |
| beam-ink on beam | 11.72 | | | |

### Category tints (provider wall chips, diagram nodes only, never for buttons)

| Token | Dark | on night | Light | on paper |
|---|---|---|---|---|
| `--tint-free` | `#7FE0A6` | 11.88 | `#17784A` | 5.08 |
| `--tint-eu` | `#AEB8FF` | 10.06 | `#3D4BC4` | 6.47 |
| `--tint-local` | `#E2AEF0` | 10.46 | `#8A3AA3` | 6.04 |
| `--tint-router` | `#D9CFB4` | 12.26 | `#6B5E3A` | 5.91 |

Chips use the tint at 100% for text and 12% for the chip fill. Tints never carry meaning alone: every chip also has a text label.

### Light mode

| Token | Hex |
|---|---|
| `--night` → paper | `#F5F6F8` |
| `--surface-1` | `#FFFFFF` (nav at 80% + blur) |
| `--surface-2` | `#FFFFFF` with `--line` border |
| `--surface-3` | `#EDEFF3` |
| `--line` | `rgba(17,21,29,0.08)` |
| `--line-strong` | `rgba(17,21,29,0.16)` |
| `--ink` | `#11151D` (16.90 on paper) |
| `--ink-muted` | `#4B5463` (7.07) |
| `--ink-faint` | `#5E6777` (5.27) |
| `--beam` | `#0A6FA3` (5.09 on paper; white text on it 5.50) |
| `--beam-ink` | `#FFFFFF` |
| `--beam-glow` | `rgba(10,111,163,0.10)` |

Banned: anything in the amber/orange family (`#f5a524` and neighbours), rainbow or multi-stop gradients, decorative gradient washes. The only gradients allowed are single-hue radial light (`--beam-glow` to transparent).

## Typography

Two families, both SIL OFL, self-hosted as subset WOFF2 from `website/public/fonts/` (latin + latin-ext), `font-display: swap`, preloaded. No CDN.

- **Mona Sans** (variable: `wght` 200-900, `wdth` 75-125): display and body.
- **Monaspace Neon** (variable `wght`): code, commands, `kbd`, the install one-liner. Nowhere else; no mono for small labels or stats.

Headlines use the width axis as part of the voice: display sizes are set slightly wide (`wdth` 112) and heavy, body is normal width. That wide-heavy setting is the type signature; don't add italics or colored words inside headlines.

| Token | Size / line-height | Weight | Width | Tracking | Use |
|---|---|---|---|---|---|
| `display` | clamp(44px, 7vw, 88px) / 0.98 | 760 | 112 | -0.035em | hero headline only |
| `h1` | clamp(36px, 4.6vw, 56px) / 1.04 | 720 | 110 | -0.03em | section titles |
| `h2` | 28px / 1.15 | 660 | 104 | -0.02em | sub-sections, big cards |
| `h3` | 20px / 1.3 | 620 | 100 | -0.01em | card titles |
| `body-lg` | 19px / 1.6 | 400 | 100 | 0 | hero lede, section intros |
| `body` | 16px / 1.65 | 400 | 100 | 0 | default |
| `small` | 14px / 1.5 | 450 | 100 | 0.005em | captions, footer |
| `code` | 14.5px / 1.55 | 450 | n/a | 0 | Monaspace Neon |

Rules: sentence case everywhere (no all-caps labels, no tracked-out eyebrows). Body measure max 68ch; lede max 54ch. Numbers in the stats row use `font-variant-numeric: tabular-nums`.

## Spacing, layout, radii

**Spacing scale (4px base):** `1`=4 `2`=8 `3`=12 `4`=16 `6`=24 `8`=32 `12`=48 `16`=64 `24`=96 `32`=128. Section vertical padding: 128 desktop, 96 tablet, 64 phone.

**Grid:** 12 columns, max content width 1200px, column gap 24px. Side gutter 16px (<640), 32px (640-1024), 48px (>1024). Content is **left-aligned**; only the FAQ intro and the final download CTA are centered.

**Radii follow hierarchy, not one value for everything:**

| Token | px | Use |
|---|---|---|
| `r-xs` | 4 | kbd, chips, badges |
| `r-sm` | 8 | buttons, inputs, tabs |
| `r-md` | 12 | small cards, code blocks |
| `r-lg` | 18 | bento cards |
| `r-xl` | 24 | app window chrome, the beam diagram stage |

Nested elements use the parent radius minus the padding (so a 12px card inside an 18px card with 6px inset).

## Depth

- **Background texture:** a 24px dot grid at `--line` opacity, masked to fade out toward the page edges. No noise images.
- **Card shine:** 1px inner top highlight `inset 0 1px 0 rgba(233,237,244,0.05)` plus the `--line` border. No drop shadow on cards in dark mode.
- **Shadows (only for things that float):**
  - `shadow-pop`: `0 8px 24px -8px rgba(0,0,0,0.5), 0 0 0 1px var(--line)` (popovers, toast, copy tooltip)
  - `shadow-window`: `0 40px 80px -24px rgba(0,0,0,0.65), 0 12px 24px -12px rgba(0,0,0,0.5)` (hero screenshot)
  - Light mode: same geometry, black at 0.12 / 0.08.
- **Glass:** only the sticky nav: `--surface-1` at 72% + `backdrop-filter: blur(14px) saturate(140%)`. Nothing else is glass.

## Motion

| Token | Value | Use |
|---|---|---|
| `dur-1` | 150ms | hover, press, focus |
| `dur-2` | 220ms | tabs, toggles, chips, copy checkmark |
| `dur-3` | 320ms | reveals, card expand, toast in |
| `dur-4` | 400ms | beam draw, nav condense |
| `ease-out` | `cubic-bezier(0.22, 1, 0.36, 1)` | entering, most UI |
| `ease-in-out` | `cubic-bezier(0.65, 0, 0.35, 1)` | moving between states (tab indicator) |
| `ease-in` | `cubic-bezier(0.55, 0, 1, 0.45)` | leaving |
| `marquee` | 60s linear infinite | provider marquee |
| `count-up` | 900ms ease-out | stats numbers, once, when 50% visible |

Rules:
- Animate only `transform`, `opacity`, `clip-path`, `stroke-dashoffset`. No layout properties.
- Scroll reveals: once per section, 8px rise + fade over `dur-3`, children staggered 40ms max 4 items. Never re-trigger.
- The one orchestrated moment is the beam: on first view of the signature section the main beam draws from laptop to provider (`dur-4` x 2), then idles with a slow pulse travelling along it (4s). Toggling adds/removes beams with `dur-4`.
- Hover: buttons and links shift color/background only (no lift, no scale); cards brighten their border to `--line-strong`. Press: `scale(0.98)` for `dur-1`.
- No bounce/spring overshoot, no parallax, no cursor-following effects.

**`prefers-reduced-motion: reduce`:** every duration drops to 0, reveals are skipped (content visible), the marquee becomes a static wrapped grid, count-ups show final numbers, beams render fully drawn with no travelling pulse, the Tool Maker demo shows its final frame with a "Play" button that steps through frames on click instead of animating, the provider-ception loop shows a static diagram.

## Components (key specs)

- **Primary button:** `--beam` fill, `--beam-ink` text, 44px tall, 20px horizontal padding, `r-sm`, weight 600. Label says what happens ("Download for Windows"). No trailing arrows.
- **Secondary button:** transparent, `--line-strong` border, `--ink` text.
- **Focus ring (everything focusable):** `outline: 2px solid var(--beam); outline-offset: 3px`; never removed.
- **kbd:** Monaspace Neon 12.5px, `--surface-3` fill, `--line-strong` border, 1px bottom border extra (keycap), `r-xs`, 2px 6px padding.
- **Copy button:** icon button inside code blocks; on click swaps to a check for 1.6s with `dur-2`; announces "Copied" via an `aria-live` region.
- **Hero meta line:** version, OS support and "no account" as three separate quiet chips, not a dot-joined string.
- **Nav:** 64px tall at top, condenses to 52px after 24px of scroll (`dur-4`), logo left, links center-left, theme toggle + download right.
- **Window chrome (screenshots):** `--surface-3` title bar 36px with three 10px circles in `--line-strong` (no traffic-light colors), `r-xl`, `shadow-window`, rotated `-2deg` / `perspective` tilt on desktop only, flat on phones.

## Layout sketches

Hero (desktop):
```
+--------------------------------------------------------------+
| nav                                                          |
|                                                              |
|  Display headline,              .  ' beam-glow cone '  .     |
|  two lines, wide + heavy      +------------------------+     |
|                               |  app screenshot        |     |
|  lede, max 54ch               |  (tilted window)       |     |
|  [Download for Windows] [Linux]|                       |     |
|  (v0.x) (Win + Linux) (no acct)+------------------------+    |
+--------------------------------------------------------------+
```
Phone: headline, lede, buttons, chips, then the screenshot flat and full-bleed minus gutter.

Signature section ("what leaves your computer"):
```
+--------------------------------------------------------------+
|  h1 + one-line intro (left)                                  |
|  +--------------------------------------------------------+  |
|  |  [laptop] ===== beam =====>  [provider you picked]     |  |
|  |      :                                                  |  |
|  |      : (dim) web search / updates / MCP nodes           |  |
|  +--------------------------------------------------------+  |
|  [ ] Web search   [ ] Update checks   [ ] MCP server         |
|  plain-words list of exactly what is sent, updates live      |
+--------------------------------------------------------------+
```
Stage is an inline SVG; toggles are real `<button role="switch">`; the text list below is the accessible source of truth, the SVG is `aria-hidden`.

Bento (desktop 12-col): one 7-col tall card (Tool Maker mini demo) + 5-col stack of two; then a row of 4 + 4 + 4. Each card shows a small real UI fragment, not an icon.

## Review against the defaults (what I changed and why)

First pass, then revised:

1. **Base color.** First draft was neutral `#0B0B0B` with an electric lime accent. That's the "near-black + acid green" default almost to the hex. Changed to an ink-blue night (`#0D1017`) and a pale cool lamplight accent tied to the beam concept, so the color has a reason.
2. **Hero.** First draft was a centered giant headline with a three-number stats row under it, the default treatment. Changed to a left-aligned split where the screenshot is lit by the beam cone; the stats row moves further down the page as its own small section.
3. **Labels.** First draft had small mono labels above every section title. Dropped: sections open with their title. Mono is reserved for real code and commands.
4. **Meta strings.** The brief's "tiny line of version/OS/no-account" was going to be a dot-joined string. Changed to three separate chips.
5. **Cards.** First draft used one 16px radius and one soft shadow on everything. Changed to a radius scale by hierarchy and no shadow on cards in dark mode; only floating things get shadows.
6. **Motion.** The brief asks for scroll reveals and hover states on every interactive element, so those stay, but kept minimal (8px, once, color-only hovers) so the beam draw is the only moment that reads as "animation".

## Originality guardrails (vs `reference/`)

Never open the reference while writing CSS or copy. Before committing any section, check: no shared class names, no shared section titles or taglines, no amber/orange, no matching layout proportions. Results go in `website/ORIGINALITY.md` at the end of milestone 9.
