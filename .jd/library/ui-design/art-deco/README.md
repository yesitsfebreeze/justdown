# Art Deco

A frontend design-style reference for the 1920s–30s Art Deco aesthetic — the gilded geometry of the Gatsby era.

## Origin/Era

Art Deco emerged at the *Exposition Internationale des Arts Décoratifs et Industriels Modernes* in Paris, 1925, and dominated visual culture through the 1920s and 1930s. It was the look of the Jazz Age and the Roaring Twenties: the Chrysler Building's stainless-steel sunburst crown, the Chicago Board of Trade, ocean liners like the *Normandie*, cinema marquees, and Cassandre's travel posters. Born of post-WWI optimism and machine-age confidence, it married luxury craftsmanship with industrial precision — lacquer, chrome, exotic woods, and gold leaf rendered into rigid geometric order. It is glamour with a straightedge.

## Defining traits

- **Symmetry and order.** Compositions are axially balanced, often mirrored left-to-right, with strong central focal points.
- **Geometric ornament.** Chevrons, zigzags, sunbursts, fans, stepped (ziggurat) forms, and concentric arcs are the core vocabulary.
- **Metallic luxury.** Gold, brass, and bronze gilding against deep blacks and midnight navy; jade and emerald as jewel accents.
- **High-contrast display type.** Tall, elegant, geometric or refined serif letterforms — uppercase, generously letter-spaced, often paired with thin hairline rules.
- **Thin gilded frames.** Fine single or double lines outline panels; borders are restrained, never heavy.
- **Stepped elevation.** Depth is suggested by nested/stepped outlines and tiered borders rather than soft drop shadows.
- **Vertical emphasis.** Skyscraper-inspired upward thrust, fluting, and tall narrow proportions.

## When to use

- Luxury, hospitality, spirits, jewelry, and fashion brands wanting timeless glamour.
- Event identities — galas, theaters, cinemas, speakeasy-themed venues, weddings.
- Editorial or portfolio sites where elegance and craftsmanship are the message.
- Any product that benefits from a sense of curated, hand-finished opulence.

## Pitfalls

- **Ornament overload.** Stacking every motif (chevron + sunburst + fan + zigzag) on one screen reads as kitsch. Choose one or two and repeat.
- **Poor contrast/accessibility.** Gold-on-black can fail WCAG contrast for body text. Reserve metallics for large headings and ornament; use near-white for reading copy.
- **Wrong gold.** Flat `#FFD700` looks like a child's crayon. Use muted, slightly desaturated brass tones and gradients to suggest metal.
- **Breaking symmetry.** Ragged, asymmetric layouts undermine the whole language. Center and mirror.
- **Heavy soft shadows.** Blurry material-style drop shadows feel anachronistic. Prefer crisp stepped outlines and subtle inner glow.
- **Too many fonts.** One display face + one body serif. More fragments the elegance.

## Token cheat-sheet

### Palette (hex)

| Role | Hex | Notes |
| --- | --- | --- |
| Background (ink) | `#0B0E14` | Near-black, faint navy cast |
| Surface / panel | `#11151F` | Raised midnight navy |
| Surface alt | `#161B28` | Card interior |
| Gold (primary metallic) | `#C8A24B` | Muted brass, never neon |
| Gold light (highlight) | `#E8CE8A` | Gradient top stop |
| Gold deep (shade) | `#9A7B2E` | Gradient bottom stop |
| Emerald / jade accent | `#1E6F5C` | Jewel accent |
| Jade light | `#3FA889` | Hover/active jade |
| Text primary | `#F2E9D8` | Warm ivory for reading |
| Text muted | `#A9A48F` | Captions, labels |
| Hairline (gilded line) | `rgba(200,162,75,0.45)` | Thin frames/dividers |

### Border-radius

- Base: `0` — Art Deco is rectilinear. Sharp corners are the default.
- Optional chamfer via `clip-path` for stepped/cut corners (e.g. `polygon()` with 45° notches).
- Pills/circles only for medallion sunburst ornaments (`border-radius: 50%`).

### Shadow recipe

Avoid soft material shadows. Build depth with stacked thin outlines + faint glow:

```css
/* stepped gilded frame (double line) */
box-shadow:
  0 0 0 1px rgba(200,162,75,0.45),   /* inner hairline */
  0 0 0 6px #0B0E14,                  /* gap */
  0 0 0 7px rgba(200,162,75,0.25),   /* outer hairline */
  0 18px 40px rgba(0,0,0,0.55);       /* grounding depth */

/* subtle gold inner glow */
box-shadow: inset 0 0 24px rgba(200,162,75,0.08);
```

### Blur/elevation

- Elevation is conveyed by **nested outlines and tiered borders**, not blur.
- Permitted blur: faint backdrop on overlays — `backdrop-filter: blur(2px)`.
- Glow accents: `filter: drop-shadow(0 0 6px rgba(200,162,75,0.35))` on ornaments only.
- Elevation scale: `flat → +1px hairline → +stepped frame → +grounding shadow`.

### Typography

- **Display:** `Cinzel` (engraved Roman serif) or `Poiret One` (geometric Deco) — uppercase headings.
- **Body:** `Cormorant Garamond` (refined high-contrast serif).
- Heading transform: `text-transform: uppercase; letter-spacing: 0.18em–0.35em`.
- Scale (modular ~1.5): `12 / 14 / 16 / 24 / 36 / 54 / 80px`.
- Weights: display 400–700; body 400–600. High stroke contrast is essential.
- Line-height: headings `1.1`, body `1.6`.

### Spacing

- Base unit: `8px`. Scale: `4 / 8 / 16 / 24 / 32 / 48 / 64 / 96`.
- Generous, symmetric padding — cards `32–48px`. Whitespace signals luxury.
- Hairline dividers separated by `≥24px`.
- Consistent vertical rhythm; center-align hero/header content.
