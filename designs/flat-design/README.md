# Flat Design

## Origin/Era

Flat design emerged roughly **2010–2013** as a reaction against the glossy, skeuomorphic UIs of the late 2000s. The landmark moments: Microsoft's **Metro** design language (Windows Phone 7, 2010; Windows 8, 2012), Google's pre-Material **flat era**, and Apple's **iOS 7** (2013), which stripped iOS of its leather, felt, and faux-3D chrome overnight. The style favored honesty to the digital medium: a screen is flat, so the interface should be too.

## Defining traits

- **No gradients** — solid, flat fills only.
- **No skeuomorphic textures** — no leather, paper, brushed metal, or faux-real materials.
- **No drop shadows or bevels** — elements sit on the same plane; depth is implied by color and space, not lighting.
- **Bold, saturated solid colors** — bright, confident, color-coded blocks.
- **Strong, simple typography** — clean geometric/humanist sans-serifs carry the hierarchy.
- **Solid color blocks** — large fields of single color define regions.
- **Sharp or lightly-rounded edges** — corners are crisp or only gently softened.
- **Generous whitespace** — breathing room replaces ornamentation.

## When to use

- Content- and data-heavy products where clarity and fast scanning matter (dashboards, admin panels, productivity apps).
- Brands wanting a modern, confident, no-nonsense voice.
- Color-coded systems (categories, statuses, navigation) where bright solids aid recognition.
- Performance-sensitive UIs — flat fills are cheap to render and scale crisply.

## Pitfalls

- **Weak affordances** — without shadows/bevels, buttons can read as flat labels. Use color, contrast, and consistent shape to signal interactivity.
- **Low contrast** — bright-on-bright or pastel-on-white can fail accessibility. Check contrast ratios.
- **Lost hierarchy** — removing depth cues means typography and spacing must do all the work; sloppy scale flattens meaning too.
- **Color overload** — too many saturated hues compete and tire the eye. Keep a disciplined palette.
- **Ambiguous state** — hover/active/disabled need clear, distinct solid colors since you can't lean on glow or elevation.

## Token cheat-sheet

**Palette**

| Role      | Hex       |
|-----------|-----------|
| primary   | `#2980B9` |
| secondary | `#16A085` |
| accent    | `#E74C3C` |
| bg        | `#ECF0F1` |
| surface   | `#FFFFFF` |
| text      | `#2C3E50` |
| muted     | `#7F8C8D` |

Hover/active variants are simply darker solids of the same hue, e.g. primary hover `#1F6391`.

**Border-radius:** `0px` (sharp) or `2–4px` (lightly rounded). No large pills unless intentional.

**Shadow recipe:** **none.** Flat design uses no drop shadows. If separation is unavoidable, use a 1px solid border or a background-color step instead of `box-shadow`.

**Blur/elevation:** none. All elements share a single z-plane; no backdrop-blur, no layered elevation.

**Typography:**
- Font family: `"Montserrat", "Open Sans", system-ui, sans-serif` (geometric sans).
- Weights: `400` (body), `600` (labels/buttons), `700` (headings).
- Sizes: `12px` caption, `14px` body, `16px` base, `20px` subhead, `28px` heading, `40px` display.
- Line-height: `1.5` body, `1.2` headings.

**Spacing scale:** `4 · 8 · 12 · 16 · 24 · 32 · 48 · 64` (px) — an 8px-based rhythm.
