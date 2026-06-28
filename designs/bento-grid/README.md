# Bento Grid

## Origin/Era

Named for the compartmentalized Japanese lunch box, the bento grid surfaced as a
named UI pattern around 2022–2023. Apple popularized it in marketing keynotes and
product pages (feature roundups on iPhone/Mac spec pages), and Microsoft, Stripe,
Vercel, and Linear adopted it across landing pages and dashboards. It is the
modern descendant of the dashboard "widget board" and Pinterest-style masonry,
tightened into a deliberate, hand-tuned grid rather than an auto-flowing wall.

## Defining traits

- A dense grid of rounded-rectangle cells, each a self-contained "box" of one idea.
- Mixed cell spans: some tiles span 2 columns and/or 2 rows; the irregular rhythm
  is the whole point — a uniform grid is not a bento.
- Generous, consistent gaps between tiles (the negative space reads as "dividers").
- Soft, low, diffuse shadows for gentle elevation; little to no hard borders.
- Large corner radii (16–24px) so every tile reads as a friendly rounded card.
- A calm, mostly neutral surface palette with one or two accent tiles for focus.
- Each tile is legible in isolation: a number, a chart, a control, a short headline.

## When to use

- Marketing/feature pages that summarize many capabilities at a glance.
- Dashboards and overview screens where heterogeneous widgets coexist.
- Profile, settings, or "home" hubs that group unrelated content compactly.
- Any place you want a premium, modern, "designed" first impression with density.

Avoid it for linear reading flows, long forms, or data tables — bento fights
sequential tasks and dense tabular data.

## Pitfalls

- Forcing equal-size tiles: kills the bento character; you just have cards.
- Too many accent/colored tiles: focus collapses, everything shouts.
- Cramming a tile with content: each box should hold one digestible idea.
- Ignoring responsive collapse: clever 2x2 spans break on narrow screens — define
  a single-column fallback.
- Inconsistent radii/gaps/shadows across tiles: the cohesion that sells the look
  depends on shared tokens.
- Over-deep shadows or hard 1px borders: makes it heavy and dated, not soft.

## Token cheat-sheet

Palette (hex):

- Page background: `#F4F4F5` (cool off-white)
- Tile surface: `#FFFFFF`
- Accent tile surface: `#6366F1` (indigo) with `#4F46E5` for pressed states
- Soft accent tile: `#EEF2FF`
- Text primary: `#18181B`
- Text secondary/muted: `#71717A`
- Hairline/divider: `#E4E4E7`

Border-radius:

- Tiles/cards: `20px`
- Inner controls (button, input): `12px`
- Pills/avatars: `999px`

Shadow recipe (soft, layered, low):

- Resting tile: `0 1px 2px rgba(24,24,27,0.04), 0 8px 24px rgba(24,24,27,0.06)`
- Hover lift: `0 2px 4px rgba(24,24,27,0.06), 0 16px 40px rgba(24,24,27,0.10)`
- Accent tile glow: `0 12px 32px rgba(99,102,241,0.35)`

Blur/elevation:

- Optional frosted overlay: `backdrop-filter: blur(12px)` over translucent white
  (`rgba(255,255,255,0.7)`).
- Elevation ladder: background (flat) → tile (resting shadow) → hovered tile (lift)
  → modal/popover (deeper, larger spread).

Typography:

- Family: `Inter`, system-ui fallback.
- Display/tile headline: `clamp(1.5rem, 3vw, 2.25rem)`, weight `700`,
  letter-spacing `-0.02em`.
- Body: `0.95rem`, weight `400`, line-height `1.5`.
- Label/eyebrow: `0.75rem`, weight `600`, uppercase, letter-spacing `0.06em`,
  muted color.

Spacing:

- Grid gap: `16px` (mobile) → `20px` (desktop).
- Tile padding: `24px` (large tiles), `20px` (small tiles).
- Control padding: `12px 16px` for buttons/inputs.
- Base spacing scale: `4 / 8 / 12 / 16 / 20 / 24 / 32 px`.
