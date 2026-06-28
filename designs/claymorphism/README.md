# Claymorphism

## Origin/Era

Coined by Michał Malewicz in early 2021 as an evolution of neumorphism. "Claymorphism" describes a puffy, 3D clay/inflated look — elements appear molded out of soft modeling clay. It rode the wave of 3D illustration trends and spread widely through Dribbble shots and design-influencer content.

## Defining traits

- Big rounded corners — generous, friendly radii.
- Puffy, inflated look, as if pumped full of air.
- Bright pastel colors.
- The signature DOUBLE inner shadow: one light highlight inset at the top, one darker shadow inset at the bottom — plus a large, soft outer drop shadow underneath.
- Playful, friendly, approachable feel.

## When to use

- Onboarding flows, kids' or education apps, and other playful, approachable products.
- Landing pages and marketing sites that want a soft, modern, friendly personality.
- Illustration-heavy interfaces where 3D clay assets already set the tone.
- Casual consumer apps (wellness, lifestyle, hobby) where warmth beats austerity.

## Pitfalls

- Can feel childish or unprofessional in serious/enterprise contexts.
- The layered shadows are easy to overdo — push too far and it looks muddy.
- Color choices are critical; the wrong pastels read cheap or garish.
- Spacing-hungry — puffy elements and big radii need lots of breathing room.

## Token cheat-sheet

Palette (pastel):
- `--bg: #E3DFFD` (soft lavender background)
- `--surface: #FFFFFF` (card / field clay)
- `--accent: #FF9DAA` (coral pink clay — primary)
- `--accent-2: #A6E3D7` (mint clay — secondary)
- `--ink: #5B5470` (muted plum text)

Border-radius scale:
- `--r-sm: 20px`
- `--r-md: 28px`
- `--r-lg: 36px`
- `--r-xl: 40px`

Shadow recipe (the clay look = 1 outer + 2 inset):
```
--clay:
  0 18px 40px -12px rgba(91, 84, 112, 0.35),   /* large soft outer drop */
  inset 0 8px 14px rgba(255, 255, 255, 0.9),    /* light highlight, top */
  inset 0 -10px 16px rgba(91, 84, 112, 0.22);   /* dark shadow, bottom */
```

Typography:
- Font family: `"Baloo 2", system-ui, sans-serif` (rounded)
- Weights: 500 body, 600 labels, 700–800 headings
- Sizes: `12px` caption, `16px` body, `20px` label/button, `32px` H1

Spacing scale: `8px / 16px / 24px / 32px / 48px` (generous, spacing-hungry).
