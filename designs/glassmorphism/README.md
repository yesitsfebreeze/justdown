# Glassmorphism

## Origin/Era
- **2007** — Windows Vista *Aero Glass*: blurred, translucent window chrome popularizes "see-through" surfaces.
- **2013** — iOS 7 introduces frosted control-center / notification panels: real-time background blur on mobile at scale.
- **2017+** — Windows 10 *Fluent Design* adds *Acrylic* material (blur + tint + noise).
- **2020** — The term **"glassmorphism"** is coined (Michał Malewicz / UX Collective) as the look trends on Dribbble; lands the same year in Apple's **macOS Big Sur** and Windows 11, cementing it as a mainstream UI style.

## Defining traits
- Translucency — surfaces let color through instead of being opaque.
- Background blur — `backdrop-filter` frosts whatever sits behind the panel (frosted glass).
- Light borders — a 1px semi-transparent white edge catches the "light".
- Multi-layer depth — stacked panels at different opacities imply z-order.
- Vivid background — a colorful gradient/photo shows through and gives the blur something to work on.
- Subtle shadows — soft, diffuse drop shadows lift panels off the page.

## When to use
- Hero sections, dashboards, and overlays sitting on top of a rich/colorful background or imagery.
- Modals, toasts, and floating navigation where you want the underlying context to stay visible.
- Marketing / product pages that want a premium, modern, "Apple-ish" feel.
- Light-on-dark or dark-on-light themes with a single strong accent.

## Pitfalls
- Contrast & accessibility — translucent fills easily fall below WCAG AA; verify text/UI contrast over the *actual* worst-case background.
- Text legibility — copy over a busy photo background becomes unreadable; add a denser tint or a solid copy layer.
- Performance — `backdrop-filter` is GPU-expensive; many large blurred layers cause jank, especially on mobile/low-end.
- Overuse — everything-is-glass kills the depth hierarchy the style depends on; reserve it for foreground surfaces.
- Fallbacks — browsers without `backdrop-filter` get a flat transparent box; provide a more opaque fallback fill.

## Token cheat-sheet
**Palette**
- Gradient bg: `#6a11cb → #2575fc → #ff5 accent`; here `#7b2ff7 0% → #f107a3 55% → #2575fc 100%`.
- Glass fill: `rgba(255, 255, 255, 0.12)` (panel), `rgba(255, 255, 255, 0.18)` (card).
- Glass border: `rgba(255, 255, 255, 0.30)`.
- Text: `#ffffff` (primary), `rgba(255,255,255,0.72)` (muted).
- Accent / primary button: `rgba(255, 255, 255, 0.9)` text on `linear-gradient(135deg,#ff8a00,#e52e71)`.

**Radius scale** — `8px` (controls) · `16px` (cards) · `24px` (panels) · `999px` (pills).

**Shadow recipe**
- Panel: `0 8px 32px rgba(31, 38, 135, 0.37)`.
- Card / button: `0 4px 24px rgba(0, 0, 0, 0.20)`.
- Inset highlight: `inset 0 1px 0 rgba(255,255,255,0.4)`.

**Blur / elevation** — `backdrop-filter: blur(16px) saturate(180%)` (panels), `blur(10px) saturate(160%)` (controls).

**Typography**
- Family: `"Inter", system-ui, sans-serif`.
- Weights: `400` body · `500` labels · `600` headings · `700` button.
- Sizes: `12px` caption · `14px` body · `16px` input · `20px` card title · `28px` heading.

**Spacing scale** — `4 · 8 · 12 · 16 · 24 · 32 · 48 px`.
