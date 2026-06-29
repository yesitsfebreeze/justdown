# Aurora Gradient

A modern mesh-gradient aesthetic built from soft, blurred multi-color glows — the luminous "aurora" backgrounds popularized by Stripe, Linear, and Vercel.

## Origin/Era

Emerged ~2020–2023 as the dominant look for developer-tooling and SaaS landing pages. Stripe's animated gradient hero (2020), Linear's dark aurora glows, and Vercel's mesh backgrounds set the template. It is the natural successor to flat design and the earlier "blurple" gradient era, made practical by wide `backdrop-filter` support and cheap GPU compositing in browsers. Closely tied to glassmorphism, which it almost always pairs with.

## Defining traits

- **Mesh / aurora background**: several large, heavily blurred radial-gradient "blobs" in adjacent hues (violet → blue → teal → pink → peach) bleeding into one another. No hard edges — the color field reads as a single soft glow.
- **Glassmorphism surfaces**: cards use `backdrop-filter: blur(...)`, a semi-transparent fill, a hairline translucent border, and a soft inner highlight so the aurora shows through.
- **Glowing gradient accents**: primary buttons are gradient-filled with a colored drop-shadow ("glow") rather than a flat shadow.
- **Large border-radius**: 16–28px on cards, pill or near-pill on buttons and inputs.
- **Diffuse, colored shadows**: low-opacity, large-blur, often tinted to the accent hue instead of pure black.
- **Clean geometric/neo-grotesque sans** (Inter, Sora, Space Grotesk), tight-ish tracking on headings, comfortable line-height on body.
- **Generous spacing**: airy padding, lots of negative space so the glow can breathe.
- Works on either a near-black or a near-white luminous base; the blobs supply the color either way.

## When to use

- Marketing/landing pages for developer tools, AI products, fintech, and SaaS.
- Hero sections, pricing pages, auth screens, and "wow" moments where atmosphere matters more than density.
- Brands that want to read as modern, premium, and technical without being cold.

## Pitfalls

- **Performance**: large blurred gradients and `backdrop-filter` are GPU-heavy. Avoid animating blur; prefer transforming pre-blurred blobs. Limit the number of frosted layers stacked on top of each other.
- **Contrast/accessibility**: text over a multi-color glow easily fails WCAG. Keep text on the solid glass surface, not directly on the mesh, and verify contrast on the lightest part of the gradient.
- **Fallbacks**: `backdrop-filter` is unsupported in some contexts — provide a more opaque solid background fallback so the card stays readable.
- **Overuse**: the look dates quickly when every element glows. Reserve glow for one or two focal accents (the primary CTA).
- **Muddy palettes**: blending too many saturated hues turns to grey-brown. Keep blobs in a tight, analogous range and let opacity, not extra colors, create depth.
- Banding on dark backgrounds — add subtle noise or keep gradients soft to mask it.

## Token cheat-sheet

### Palette (hex)
Aurora blob hues (use at 40–70% opacity, heavily blurred):
- Violet `#7C3AED`
- Indigo/Blue `#4F46E5` → `#3B82F6`
- Teal/Cyan `#2DD4BF` / `#22D3EE`
- Pink `#EC4899`
- Peach/Amber `#FB923C`

Dark base: `#0B0B12` (near-black, slightly blue).
Light base: `#F5F3FF` (faint violet white).
Glass surface: `rgba(255,255,255,0.08)` on dark / `rgba(255,255,255,0.55)` on light.
Glass border: `rgba(255,255,255,0.18)`.
Text on glass (dark): `#F4F4FA` primary, `#A9A8C0` muted.
Accent gradient: `linear-gradient(135deg, #7C3AED, #4F46E5 45%, #22D3EE)`.

### Border-radius
- Cards / large panels: `24px`
- Buttons & inputs: `12px` (or full pill `999px` for buttons)
- Small chips / badges: `8px`

### Shadow recipe
- Card lift: `0 8px 32px rgba(17, 12, 46, 0.35)` plus inner highlight `inset 0 1px 0 rgba(255,255,255,0.12)`.
- Button glow: `0 8px 24px rgba(124, 58, 237, 0.45)` (tinted to accent, large blur, no harsh offset).
- Avoid pure-black shadows; tint toward the dominant blob hue.

### Blur / elevation
- Background blobs: `filter: blur(80px–120px)` on large absolutely-positioned shapes.
- Glass cards: `backdrop-filter: blur(20px) saturate(150%)`.
- Inputs: lighter `backdrop-filter: blur(8px)`.
- Elevation is conveyed by glow intensity + translucency, not stacked opaque layers.

### Typography
- Font: Inter / Sora / Space Grotesk (geometric neo-grotesque sans).
- Headings: weight 600–700, letter-spacing `-0.02em`, line-height `1.1`.
- Body: weight 400–450, size `15–16px`, line-height `1.6`.
- Labels: weight 500, size `13px`, slightly muted color.

### Spacing
- Base unit `8px`; common steps `8 / 16 / 24 / 32 / 48 / 64`.
- Card padding: `32px–40px`.
- Section gaps: `48px+`.
- Generous, airy — let the glow show through the gaps.
