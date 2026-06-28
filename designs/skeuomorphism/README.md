# Skeuomorphism

## Origin/Era
Skeuomorphism is a design language in which digital interfaces mimic the look, texture, and behavior of their real-world physical counterparts. The term predates computing (a skeuomorph is an object retaining ornamental design cues from structures once necessary in the original), but it defined mainstream UI from roughly **2007 to 2012**, the early iPhone/iOS era under Steve Jobs and Scott Forstall. Canonical examples: iOS Notes on yellow legal-pad paper, the green felt of Game Center, the leather-stitched Calendar and Find My Friends, the wooden bookshelf of iBooks, Voice Memos' chrome microphone, and the analog-watch Clock icon. Mac OS X's brushed-metal windows and Aqua's glossy "lickable" buttons are close cousins. The style was supplanted by flat design when iOS 7 shipped in 2013.

## Defining traits
- **Real-material mimicry**: brushed metal, leather, linen, felt, wood, glass, paper.
- **Rich multi-stop gradients** simulating curved, lit surfaces — never flat fills.
- **Bevels and embossing**: edges that appear raised or carved via paired light/dark borders.
- **Dual shadows**: an outer drop shadow for elevation plus an inner shadow for recesses.
- **Glossy highlights**: a bright specular band across the top half of buttons and panels.
- **Tactile affordances**: buttons look physically pressable; sliders look like real toggles.
- **Stitching, seams, screws, and torn edges** as decorative material detail.
- **Depth realism**: consistent light source (top), occlusion, ambient shadow.

## When to use
- Audiences new to computing or touch, where a literal metaphor lowers the learning curve.
- Apps that emulate a physical tool whose conventions carry meaning (calculator, audio mixer, notepad, e-reader, instrument).
- Nostalgia, retro, or premium "crafted" branding where richness signals quality.
- Kiosks, kids' apps, and games where playful tactility aids engagement.

## Pitfalls
- **Heavy and busy**: textures and shadows compete with content and tire the eye.
- **Poor scalability**: pixel-perfect materials break across densities and dark mode.
- **Inconsistent metaphors** confuse more than they help (a leather calendar, why?).
- **Accessibility**: low text-on-texture contrast; embossing can reduce legibility.
- **Performance/weight**: stacked gradients and shadows are expensive and verbose.
- **Dated feel**: reads as early-2010s; can signal "old" outside intentional retro use.

## Token cheat-sheet

### Palette (hex)
| Role | Hex | Note |
| --- | --- | --- |
| Felt green base | `#2e6b3e` | Game Center table |
| Felt green shade | `#1f4d2b` | radial darkening at edges |
| Leather brown | `#5a3b22` | stitched panel |
| Leather highlight | `#7a5230` | top-lit grain |
| Leather shadow | `#3a2614` | recessed grain |
| Stitch thread | `#d9b98c` | dashed seam |
| Brushed metal light | `#f2f2f4` | top of bevel |
| Brushed metal mid | `#c4c8cc` | body |
| Brushed metal dark | `#8d9398` | bottom of bevel |
| Glossy blue button top | `#5aa9f5` | specular start |
| Glossy blue button bottom | `#1f6fd6` | base |
| Recessed field paper | `#fbf7ea` | legal-pad inset |
| Emboss highlight | `rgba(255,255,255,.6)` | bottom text shadow |
| Emboss shadow | `rgba(0,0,0,.55)` | top text shadow |
| Hairline divider | `rgba(0,0,0,.25)` | seams |

### Border-radius
- Buttons: `8px`–`12px` (soft, finger-friendly).
- Cards/panels: `10px`–`16px`.
- Inset fields: `8px`.
- Toggle/pill controls: `999px`.

### Shadow recipe
Raised button (outer drop + top inner highlight + bottom inner shade):
```
box-shadow:
  0 2px 3px rgba(0,0,0,.45),            /* ambient drop */
  inset 0 1px 0 rgba(255,255,255,.55),  /* top gloss bevel */
  inset 0 -2px 4px rgba(0,0,0,.35);     /* bottom rounding */
```
Recessed/inset field (carved into surface):
```
box-shadow:
  inset 0 2px 4px rgba(0,0,0,.45),      /* top inner shadow */
  inset 0 -1px 0 rgba(255,255,255,.5);  /* bottom lip catch-light */
```
Pressed state: drop the outer shadow and add `inset 0 2px 5px rgba(0,0,0,.4)`.

### Blur / elevation
- Drop-shadow blur radius `2px`–`8px`; keep it tight — surfaces sit close to the plane.
- Inner-shadow blur `2px`–`5px` for soft carved edges.
- Elevation layers: background material → panel (+drop) → control (+drop +inner) → label (emboss).
- Optional gloss overlay: a top-half `linear-gradient(rgba(255,255,255,.35), transparent)`.

### Typography
- Era UI font: **Helvetica Neue** on iOS; a humanist sans like **PT Sans** / **Open Sans** is a faithful web stand-in.
- Weights: 600–700 for headings/labels (to carry emboss), 400 for body.
- Sizes: header `20–22px`, label `13–14px`, body `15–16px`, button `16–17px` bold.
- Letter-spacing slightly tight on bold labels; always pair text with an emboss shadow.
- Emboss text: `text-shadow: 0 1px 0 rgba(255,255,255,.6);` on dark-on-light, or `0 -1px 1px rgba(0,0,0,.5)` light-on-dark.

### Spacing
- Base unit `8px`; component padding `12px`–`16px`.
- Touch targets `>=44px` tall (iOS HIG).
- Panel inner padding `16px`–`20px`; stitched border inset `6px`–`10px` from edge.
- Field height `40px`–`44px`; gap between stacked controls `12px`–`16px`.
