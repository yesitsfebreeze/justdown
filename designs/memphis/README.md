# Memphis

## Origin/Era

Memphis (a.k.a. the Memphis Group / Memphis Milano) was a design and architecture
collective founded in **Milan in 1981** by **Ettore Sottsass** with a circle of
young designers (Michele De Lucchi, Nathalie du Pasquier, George Sowden, Martine
Bedin and others). Active roughly **1981–1987**, it became the defining face of
**1980s postmodern design**, rejecting the sober "good taste" of modernism in
favor of loud color, kitsch, and decoration-for-its-own-sake. Its DNA is
everywhere in the era's graphics — most famously the *Saved by the Bell* title
card and Dorothy Triscari "squiggle"-on-everything look. Du Pasquier's printed
patterns and Sottsass's *Carlton* bookcase are the canonical references.

## Defining traits

- **Clashing high-saturation primaries**: hot pink, cyan, lemon yellow, primary
  red and blue, slammed against pure black and white with no harmonizing.
- **Geometric confetti**: scattered triangles, circles, half-circles, zigzags,
  squiggles ("Bacterio"/"squiggle" motif), and dotted/cross-hatched fields.
- **Thick black outlines** around shapes and UI elements — flat, no gradients.
- **Terrazzo / Bacterio surface patterns**: speckled chips on a light ground.
- **Asymmetry and tilt**: elements rotated a few degrees, off-grid placement.
- **Chunky geometric/display type**, often condensed or rounded, set big.
- **Flat color blocking** — depth comes from layering and hard offset shadows,
  not soft elevation.

## When to use

- Playful, youthful, expressive brands: events, music, kids' products, creative
  portfolios, party/festival sites, retro-80s/90s campaigns.
- Marketing splash pages and microsites where personality beats restraint.
- Anywhere you want energy, fun, and "anti-corporate" warmth.

## Pitfalls

- **Accessibility**: clashing colors and busy patterns wreck contrast and
  readability. Keep body text on solid high-contrast ground; reserve chaos for
  decoration. Always check WCAG contrast on text.
- **Cognitive overload**: confetti everywhere becomes noise. Use whitespace and
  let one or two shapes breathe; don't tile every surface.
- **Wrong context**: it reads as un-serious — avoid for finance, healthcare,
  enterprise, or trust-critical flows.
- **Cheap vs. designed**: random color = tacky; curated clashing palette = chic.
  Constrain to a fixed palette and a small shape vocabulary.
- **Motion sickness**: heavy patterns + animation can be physically unpleasant;
  honor `prefers-reduced-motion`.

## Token cheat-sheet

### Palette (hex)
| Token | Hex | Role |
| --- | --- | --- |
| `--ink` | `#1a1a1a` | Near-black outlines, text |
| `--paper` | `#fdf6e3` | Warm off-white ground |
| `--pink` | `#ff2e88` | Hot pink, primary accent |
| `--cyan` | `#21d4fd` | Electric cyan |
| `--yellow` | `#ffd23f` | Lemon yellow |
| `--red` | `#ff5252` | Coral/primary red |
| `--blue` | `#2962ff` | Primary blue |
| `--mint` | `#2bd9b1` | Mint green |
| `--white` | `#ffffff` | Pure white blocks |

### Border-radius
- Mixed by intent: cards/buttons `12px` (chunky-soft), inputs `8px`.
- Decorative shapes use extremes: circles `50%`, pills `999px`, sharp `0`.
- Avoid one uniform radius — variation is part of the look.

### Shadow recipe (hard offset, no blur)
```css
--shadow-pop: 6px 6px 0 0 var(--ink);     /* cards, buttons */
--shadow-pop-lg: 10px 10px 0 0 var(--ink); /* hero elements */
/* hover: translate(-2px,-2px) + shadow grows to 8px 8px 0 var(--ink) */
```

### Blur / elevation
- **No soft blur, no glassmorphism.** Elevation is faked with solid black
  hard-offset shadows and physical layering of opaque shapes.
- Stack order: pattern field (back) → shapes → outlined card → content (front).

### Typography
- Display/headings: bold geometric or rounded display face — e.g. **Fredoka**,
  Poppins, or Archivo Black. Weight `700`–`900`, generous size, slight tilt OK.
- Body: clean geometric sans (Poppins / system sans) at `400`–`500`.
- `letter-spacing` tight on headings; uppercase for labels/buttons.
- Scale: h1 `clamp(2.5rem, 6vw, 4rem)`, h2 `1.75rem`, body `1rem`, label `0.8rem`.

### Spacing
- Base unit `8px`; scale `4 / 8 / 16 / 24 / 40 / 64`.
- Thick borders: `3px`–`4px` solid `--ink` on framed elements.
- Generous padding inside cards (`24–32px`) to offset busy backgrounds.
