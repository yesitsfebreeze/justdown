# Minimalism

## Origin/Era

Minimalism descends from early-20th-century reductive movements — the Bauhaus
(1919–1933), De Stijl, and above all the Swiss / International Typographic Style
of the 1950s with its grids, sans-serif type, and asymmetric clarity. Dieter
Rams' "Weniger, aber besser" (less, but better) at Braun codified it for product
design. On the web it became dominant from the early 2010s onward as flat design
and content-first thinking displaced skeuomorphism and heavy chrome.

## Defining traits

- **Maximal whitespace** — negative space is the primary design element, not a leftover.
- **Near-monochrome palette** — neutrals only, with at most one restrained accent.
- **Strong typographic hierarchy** — meaning carried by size, weight, and spacing.
- **Very few elements** — every element earns its place; nothing decorative.
- **No ornament** — no gradients, textures, or heavy shadows; thin hairline borders instead.
- **Content-first** — the interface recedes so the content leads.
- **Calm, even rhythm** — a consistent spacing scale and alignment grid.

## When to use

- Reading- and content-heavy products: editorial, docs, blogs, knowledge bases.
- Tools where focus and low cognitive load matter (writing apps, dashboards, settings).
- Premium / editorial brands signalling restraint and confidence.
- Anywhere the content itself (photography, prose, data) should be the hero.

## Pitfalls

- **Sterile, not calm** — too little contrast or hierarchy reads as bland or unfinished.
- **Poor affordance** — stripping borders/shadows can hide what is clickable.
- **Hidden function** — minimal chrome can bury navigation and actions.
- **Accessibility** — low-contrast grays can fail WCAG; whitespace is not an excuse to skip focus states.
- **Faux-minimalism** — empty is not the same as considered; reduction must be intentional.

## Token cheat-sheet

### Palette (near-monochrome, one accent)

| Token       | Hex       | Use                                  |
|-------------|-----------|--------------------------------------|
| `--bg`      | `#fafafa` | Page background                      |
| `--surface` | `#ffffff` | Cards / raised surfaces              |
| `--text`    | `#111111` | Primary text                         |
| `--muted`   | `#6b7280` | Secondary text, labels               |
| `--hairline`| `#e5e5e5` | Hairline borders / dividers          |
| `--accent`  | `#2563eb` | Single restrained accent (links/CTA) |

### Border-radius

- `--radius: 6px` (subtle, never pill-shaped or playful)

### Shadow recipe (minimal/subtle)

- Default: **none** — prefer a `1px` hairline border (`1px solid #e5e5e5`).
- When elevation is unavoidable: `0 1px 2px rgba(17, 17, 17, 0.05)`.

### Blur / elevation

- No backdrop blur, no glass. Elevation is communicated by hairlines and spacing, not depth.
- Max elevation tier: `0 1px 3px rgba(17, 17, 17, 0.06)`.

### Typography

- Family: `"Inter", system-ui, sans-serif` (clean grotesque).
- Weights: `400` (body), `500` (labels/buttons), `600` (headings) — avoid heavy 700+.
- Sizes: `13px` caption, `15px` body, `20px` subhead, `32px` / `48px` display.
- Line-height: generous — `1.6` for body, `1.2` for display.
- Letter-spacing: `-0.01em` on large headings; otherwise default.

### Spacing scale (whitespace-forward)

- `4 · 8 · 16 · 24 · 40 · 64 · 96 px` — lean on the larger steps.
- Default rhythm between blocks: `24px`+; section padding: `64px`+.
