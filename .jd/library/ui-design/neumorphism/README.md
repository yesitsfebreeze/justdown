# Neumorphism (Soft UI)

## Origin/Era

Coined and popularized by Alexander Plyuto and Michał Malewicz around December 2019 into 2020, surfacing through viral Dribbble concepts. Often called "soft UI", it is a hybrid of skeuomorphism and flat design — elements appear molded from a single continuous surface rather than layered on top of it.

## Defining traits

- Monochromatic: background and elements share the exact same base color.
- Dual shadows: one dark shadow bottom-right and one light shadow top-left, simulating a single light source.
- The shadow pair produces an extruded (raised) or embossed (pressed) look.
- Deliberately low contrast between surface and elements.
- Soft, generously rounded shapes.
- Two core states: "raised" (drop shadows) vs "pressed"/inset (inner shadows).

## When to use

- Calm, minimal dashboards, music/media players, and concept/portfolio pieces.
- Single-surface layouts where every control lives on one flat color.
- Toggle- and slider-heavy interfaces where the raised/pressed metaphor reads naturally.
- Light-mode product showcases prioritizing aesthetic over dense information.

## Pitfalls

- SEVERE accessibility/contrast problems — low contrast routinely fails WCAG; the dominant reason not to ship it.
- Low affordance: raised buttons barely look clickable; pressed and disabled states are easily confused.
- Breaks on busy or multi-color schemes — it needs a single flat surface color to work.
- Only functions on one base surface color; gradients, images, or layered backgrounds destroy the effect.

## Token cheat-sheet

| Token | Value |
|---|---|
| Base surface | `#e0e5ec` |
| Light shadow | `#ffffff` |
| Dark shadow | `#a3b1c6` |
| Text primary | `#4d5b6e` |
| Text muted | `#8a98ab` |
| Accent | `#6d8bbf` |

Border-radius scale: `12px` (small), `20px` (card/field), `50px` (pill button).

Box-shadow recipes:

```css
/* Raised */
box-shadow: -6px -6px 12px #ffffff, 6px 6px 12px #a3b1c6;
/* Pressed / inset (form field) */
box-shadow: inset -4px -4px 8px #ffffff, inset 4px 4px 8px #a3b1c6;
```

Typography: `Poppins`, weights 400 / 500 / 600. Sizes: `13px` label, `15px` body/input, `22px` card title, `28px` header.

Spacing scale: `8px / 16px / 24px / 32px`.
