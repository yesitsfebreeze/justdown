# Neo-Brutalism

## Origin/Era

Neo-brutalism (a.k.a. "neubrutalism") surfaced in web/UI design around 2020–2022,
peaking in 2022–2023. It borrows its name and attitude from architectural
Brutalism (béton brut, "raw concrete") and from the earlier "brutalist web
design" movement (~2014–2018) that rejected polished, templated sites in favor
of raw, unstyled HTML. The neo- flavor keeps the rawness but adds deliberate,
loud styling: it is a reaction against the soft, homogenous "corporate Memphis"
and glassy neumorphism/material aesthetics of the late 2010s. Popularized by
indie products, Gumroad's 2022 redesign, design-tool marketing sites, and the
Tailwind/Figma community.

## Defining traits

- Thick, solid, pure-black borders (3–4px+) on nearly every element.
- Hard offset drop shadows with zero blur — a solid color block offset down-right.
- Raw, high-saturation flat colors: electric yellow, hot pink, lime, cyan, on
  off-white or pure-white backgrounds. No gradients, no transparency.
- Chunky geometric/grotesque sans-serif type, heavy weights, often uppercase.
- Sharp corners: zero or near-zero border-radius.
- Visible, tactile interaction — buttons physically "press" by translating
  toward their shadow on `:active`/`:hover` and the shadow shrinks.
- Intentional rawness and high contrast over subtlety; layout grids exposed
  rather than hidden.

## When to use

- Indie products, portfolios, landing pages, and brands that want to feel bold,
  playful, memorable, and anti-corporate.
- Marketing/launch pages where standing out beats blending in.
- Design-forward tools, creative communities, and dev-tool branding.
- Anywhere personality and high contrast (good for accessibility contrast
  ratios) matter more than calm minimalism.

## Pitfalls

- Fatigue and noise: loud everywhere means nothing stands out; reserve accents.
- Accessibility traps despite high contrast — saturated color pairs can fail
  WCAG, and "decorative chaos" can hurt readability and focus order.
- Poor fit for data-dense, enterprise, or trust-critical UIs (finance, health).
- Hard shadows and thick borders eat layout space; can break on small screens.
- Trend risk: strongly dated to its era; can read as gimmicky if overdone.
- Hover/press translate effects must not shift layout or cause reflow jank.

## Token cheat-sheet

Palette (hex):

| Token            | Hex       | Use                          |
| ---------------- | --------- | ---------------------------- |
| `--ink`          | `#111111` | borders, shadows, text       |
| `--paper`        | `#fafafa` | page background              |
| `--surface`      | `#ffffff` | cards / surfaces             |
| `--accent`       | `#ffde00` | electric yellow, primary bg  |
| `--accent-2`     | `#ff5470` | hot pink                     |
| `--accent-3`     | `#00e0c6` | cyan / mint                  |
| `--accent-4`     | `#7c4dff` | violet                       |

- Border-radius: `0px` (sharp); at most `2px`–`4px` if softening is required.
- Border: `3px` (controls/inputs) to `4px` (cards/buttons) `solid var(--ink)`.
- Shadow recipe (hard, zero blur): `box-shadow: 6px 6px 0 0 var(--ink);`
  Larger surfaces use `8px 8px 0 0`. Never use blur or spread for elevation.
- Blur/elevation: blur radius is always `0`. "Elevation" is expressed purely by
  the offset distance of the hard shadow (e.g. `4px` → `6px` → `8px`), not by
  soft blur or opacity. No backdrop-blur, no glass.
- Typography: geometric/grotesque heavy sans (e.g. "Space Grotesk", "Archivo",
  "Inter") weights `700`–`800`; headings often `uppercase`, `letter-spacing`
  `-0.01em` to `0`; base size `16px`, scale `1.25`.
- Spacing: `8px` base unit — `4 / 8 / 16 / 24 / 32 / 48px`. Generous padding
  inside chunky borders (cards `24–32px`, buttons `12–16px`).
