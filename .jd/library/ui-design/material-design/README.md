# Material Design

Google's open-source design system. This entry targets **Material 3 (Material You)** — the 2021+ generation that introduced dynamic color, tonal surfaces, and the large-radius shape scale. Earlier generations (Material 1 / 2014, Material 2 / 2018) share the metaphor but use flatter palettes, harder shadows, and smaller corners.

## Origin/Era

Introduced by Google at I/O 2014 as "Material Design," a unifying language for Android, the web, and Google's own products. Material 2 landed in 2018 (theming, more restraint). **Material 3 / Material You** debuted with Android 12 in 2021: user-personalized "dynamic color" derived from the wallpaper, tonal elevation, and softer, friendlier geometry. The core metaphor has always been *material* — surfaces behave like sheets of tangible, layered paper-and-ink under a real light source, casting shadows and moving along a z-axis.

## Defining traits

- **Tonal elevation.** In M3, depth is communicated primarily by *surface tone* — higher surfaces get a stronger primary-tinted overlay — backed by soft, diffuse shadows rather than the hard drop shadows of M2.
- **Dynamic / tonal color roles.** Color is expressed as semantic *roles* (primary, on-primary, primary-container, surface, surface-variant, outline…) generated from tonal palettes, not as ad-hoc hexes. Each role guarantees an accessible foreground pairing.
- **Large, rounded geometry.** A shape scale runs from extra-small to full. Cards use medium radii (~12dp); buttons and chips use the *stadium* / pill ("full") radius; FABs use large radii.
- **Roboto type scale.** A consistent display → headline → title → body → label ramp, weighted for legibility at every size.
- **Ripple emphasis.** Touch and hover are acknowledged with an ink-ripple state layer that radiates from the contact point and a translucent state overlay.
- **Signature components.** Filled / tonal / outlined / text buttons, the Floating Action Button (FAB), elevated & filled cards, the top app bar, and outlined or filled text fields with a label that floats into the border notch on focus.

## When to use

- Android apps and cross-platform products that should feel native to Google's ecosystem.
- Teams that want a complete, documented, accessible component spec out of the box.
- Products that benefit from personalization (wallpaper-driven dynamic color) or need a calm, content-forward, highly legible surface system.

## Pitfalls

- **Generic "Google app" look.** Used straight from defaults, M3 reads as un-branded. Override the seed/primary color and type to differentiate.
- **Over-elevation.** Stacking many high-elevation surfaces muddies hierarchy; reserve strong tonal overlays for the few things that truly float.
- **Mixing M2 and M3.** Hard M2 shadows next to M3 tonal surfaces, or small radii beside stadium buttons, breaks the language. Commit to one generation.
- **Ignoring color roles.** Hand-picking hexes instead of using on-/container roles breaks contrast guarantees and dark-mode behavior.
- **Density.** The 8dp grid and large touch targets are roomy; cramming desktop-dense tables into it fights the system.

## Token cheat-sheet

### Palette — M3 color roles (light scheme, baseline purple seed)

| Role | Hex |
| --- | --- |
| primary | `#6750A4` |
| on-primary | `#FFFFFF` |
| primary-container | `#EADDFF` |
| on-primary-container | `#21005D` |
| secondary | `#625B71` |
| secondary-container | `#E8DEF8` |
| surface | `#FEF7FF` |
| surface-variant | `#E7E0EC` |
| surface-container | `#F3EDF7` |
| surface-container-high | `#ECE6F0` |
| on-surface | `#1D1B20` |
| on-surface-variant | `#49454F` |
| outline | `#79747E` |
| outline-variant | `#CAC4D0` |
| error | `#B3261E` |
| background | `#FEF7FF` |

Dark scheme flips to: surface `#141218`, on-surface `#E6E0E9`, primary `#D0BCFF`, on-primary `#381E72`.

### Border-radius — M3 shape scale

| Token | Radius |
| --- | --- |
| none | `0` |
| extra-small | `4px` |
| small | `8px` |
| medium | `12px` (cards) |
| large | `16px` (FAB) |
| extra-large | `28px` (dialogs, large FAB) |
| full / stadium | `9999px` (buttons, chips) |

### Shadow recipe — elevation levels (dp → box-shadow)

Each level pairs a *key* (umbra) and *ambient* shadow. In M3 these are intentionally soft and low-opacity, complemented by a tonal surface overlay.

| Level | box-shadow |
| --- | --- |
| 0dp | `none` |
| 1dp | `0 1px 2px rgba(0,0,0,.30), 0 1px 3px 1px rgba(0,0,0,.15)` |
| 2dp | `0 1px 2px rgba(0,0,0,.30), 0 2px 6px 2px rgba(0,0,0,.15)` |
| 3dp | `0 1px 3px rgba(0,0,0,.30), 0 4px 8px 3px rgba(0,0,0,.15)` |
| 4dp | `0 2px 3px rgba(0,0,0,.30), 0 6px 10px 4px rgba(0,0,0,.15)` |
| 5dp | `0 4px 4px rgba(0,0,0,.30), 0 8px 12px 6px rgba(0,0,0,.15)` |

### Blur / elevation overlay

- **Tonal overlay:** tint the surface toward `primary` as elevation rises — roughly 5% / 8% / 11% / 12% / 14% primary blended over `surface` for levels 1–5. This is the dominant depth cue in M3.
- **Scrim:** `rgba(0,0,0,.32)` behind modal surfaces.
- **State layer:** overlay `on-surface`/`primary` at 8% (hover), 12% (focus/pressed) on interactive elements.
- **Backdrop blur** is not core to M3 (unlike glassmorphism); use sparingly if at all.

### Typography — Roboto type scale

Font family: `Roboto` (fallback `system-ui, sans-serif`). Roboto Flex / variable for fine weight control.

| Style | Size / line-height / weight |
| --- | --- |
| display-large | 57 / 64 / 400 |
| display-medium | 45 / 52 / 400 |
| headline-large | 32 / 40 / 400 |
| headline-medium | 28 / 36 / 400 |
| title-large | 22 / 28 / 400 |
| title-medium | 16 / 24 / 500 |
| body-large | 16 / 24 / 400 |
| body-medium | 14 / 20 / 400 |
| label-large | 14 / 20 / 500 (button text) |
| label-small | 11 / 16 / 500 |

### Spacing — 8dp grid

- Base unit **8dp**; half-step **4dp** for fine alignment.
- Common steps: `4 · 8 · 12 · 16 · 24 · 32 · 48 · 64`.
- Screen margins ~16dp (compact) to 24dp; card padding 16dp; min touch target **48×48dp**.
