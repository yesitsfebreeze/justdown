# Dark Mode

OLED-first dark UI: true-black backgrounds, elevation by surface tint, off-white text, desaturated accents.

## Origin/Era

Dark interfaces long predate the modern convention (CRT terminals, early IDEs were dark by necessity). But *systematic* OS-level dark mode arrived ~2018–2019: macOS Mojave (2018), iOS 13 and Android 10 (2019), and Material Design's dark theme guidance. OLED-first thinking — pushing backgrounds to true black to exploit per-pixel emissive displays — became mainstream alongside OLED phones in the same window.

## Defining traits

- **True-black canvas** (`#000000` or near) — on OLED, black pixels are physically off, saving power and yielding effectively infinite contrast.
- **Elevation by lightening, not shadow** — higher surfaces are *lighter* gray overlays. Drop shadows are nearly invisible on black, so depth is communicated by surface tint (the Material "elevation overlay" model).
- **Off-white text, never pure white** — `#E0`–`#F0` range. Pure `#FFFFFF` on `#000000` causes halation/blooming and eye strain.
- **Desaturated accents** — saturated hues vibrate against black. Pull saturation/lightness down so the accent reads calm.
- **Hairline separators** — 1px low-opacity white borders instead of heavy lines; subtle focus glows instead of hard outlines.

## When to use

- Low-light / night usage, media and reading apps, dashboards, dev tools.
- OLED devices where battery and contrast genuinely benefit.
- As a respectful default for users who set system dark mode.

## Pitfalls

- **Pure white text on pure black** — halation, ghosting, fatigue. Use off-white.
- **Reusing light-mode shadows** — invisible on black; depth collapses. Use surface tints.
- **Over-saturated accents** — chromatic aberration / vibration at the edge. Desaturate.
- **Inverting, not redesigning** — naive color inversion breaks imagery and contrast ratios.
- **Too many elevation levels** — surfaces become muddy grays; keep 3–4 steps max.
- **Ignoring contrast minimums** — off-white must still clear WCAG AA against each surface.

## Token cheat-sheet

**Palette (OLED-first)**

| Token | Value | Role |
|---|---|---|
| `--bg` | `#000000` | true-black page canvas |
| `--surface-1` | `#0E0E10` | base card / panel |
| `--surface-2` | `#16161A` | raised surface (header, inputs) |
| `--surface-3` | `#1E1E24` | hover / highest elevation |
| `--border` | `rgba(255,255,255,0.08)` | hairline separators |
| `--text` | `#ECECEE` | primary off-white (not `#FFF`) |
| `--text-muted` | `#9A9AA2` | secondary / labels |
| `--accent` | `#7C9DF0` | desaturated periwinkle-blue |
| `--accent-strong` | `#6B8DE8` | accent hover |
| `--on-accent` | `#0A0A0C` | text on accent fill |

**Elevation model** — In dark UI, depth is *not* a drop shadow (a shadow over black is invisible). Each step up the elevation ladder lays a slightly lighter white overlay over the canvas, so `surface-1` → `surface-2` → `surface-3` read as progressively raised. Optional faint glow `0 0 0 1px var(--border)` defines the edge.

**Border-radius** — `4px` (inputs/chips), `10px` (cards), `8px` (buttons), `999px` (pills).

**Shadow recipe** — Prefer surface tint for elevation. If a shadow is used at all, keep it soft and dark to seat an overlay (e.g. menus): `0 8px 24px rgba(0,0,0,0.6)`. Pair with a `1px` hairline ring so the edge is legible on black.

**Blur** — Overlay scrims / glass: `backdrop-filter: blur(12px)` over `rgba(20,20,24,0.6)`.

**Typography** — Family: `Inter`, system-ui fallback. Weights: 400 body, 500 labels/buttons, 600 headings. Sizes: `12px` caption, `14px` body, `16px` input, `18px` card title, `22px` page title. Line-height `1.5` body, `1.2` headings. Letter-spacing `-0.01em` on headings.

**Spacing scale** — `4 · 8 · 12 · 16 · 24 · 32 · 48` px (4px base).

**Focus glow** — `box-shadow: 0 0 0 3px rgba(124,157,240,0.30); border-color: var(--accent)`.
