# Retro Pixel

## Origin/Era

The 8/16-bit videogame UI aesthetic of the mid-1980s through early 1990s: the NES (1983), Game Boy (1989), SNES/Genesis (1990–91), and the arcade cabinets of the era. Born from hard hardware limits — fixed tile grids, tiny palettes, no anti-aliasing, no sub-pixel rendering. What was once a constraint is now a deliberate style: chunky pixel fonts, nearest-neighbor scaling, dithered gradients, and blocky "pressable" chrome.

## Defining traits

- **Pixel typography** — bitmap-style fonts (e.g. "Press Start 2P", "Pixelify Sans") rendered crisp, never smoothed.
- **Hard edges everywhere** — `border-radius: 0`. Corners are pixels, not curves.
- **Limited palette** — a tight, named set of colors (NES/Game Boy-inspired), used flatly with no gradients (or dithered fake-gradients).
- **No anti-aliasing** — `image-rendering: pixelated` so scaled art stays blocky.
- **Hard offset drop shadows** — solid color block shadows with **zero blur**, offset by whole-pixel multiples.
- **Pixel-art accents** — icons and decorations built from squares (box-shadow tricks or `shape-rendering: crispEdges` SVG).
- **Chunky pressable buttons** — thick borders, a hard shadow that collapses on `:active` to fake a physical press.
- **Grid-snapped spacing** — everything aligns to a base pixel unit (4px/8px).

## When to use

- Game tools, game launchers, retro/arcade brands, and chiptune/demoscene projects.
- Playful productivity apps, hackathon UIs, and dashboards that want personality.
- Landing pages or portfolios where a strong nostalgic identity beats neutral polish.
- Anywhere a deliberate, memorable "this is a toy/game" signal is a feature, not a bug.

## Pitfalls

- **Legibility** — pixel fonts are wide and tiring; never use them for long body copy. Reserve for headings, labels, and short UI text; set generous line-height.
- **Accessibility** — limited palettes easily fail contrast ratios. Verify WCAG contrast; don't rely on color alone.
- **Scaling artifacts** — bitmap fonts and pixel art only look right at integer scales. Avoid fractional zoom and odd font sizes; snap to the grid.
- **Overuse** — full-screen pixel everything reads as a gimmick. Anchor it with whitespace and restraint.
- **Performance of fake shadows** — large box-shadow pixel-art sprites can get heavy; prefer crisp SVG for anything detailed.

## Token cheat-sheet

```css
:root {
  /* Palette — NES / Game Boy-inspired limited set */
  --rp-bg:        #0f0f1b; /* Void (near-black blue)   */
  --rp-surface:   #1d2b53; /* Deep Sea (panel/card)     */
  --rp-ink:       #fff1e8; /* Bone White (text)         */
  --rp-primary:   #ff004d; /* Arcade Red (primary CTA)  */
  --rp-accent:    #ffec27; /* Coin Gold (highlights)    */
  --rp-success:   #00e436; /* 1-Up Green                */
  --rp-info:      #29adff; /* Sky Blue                  */
  --rp-shadow:    #000000; /* Hard Black (drop shadows) */
  --rp-muted:     #5f574f; /* Stone Gray (borders/dim)  */

  /* Border-radius — hard pixel corners, always */
  --rp-radius: 0;

  /* Border — thick blocky frames */
  --rp-border: 4px solid var(--rp-ink);

  /* Shadow recipe — hard offset block, NO blur */
  --rp-shadow-1: 4px 4px 0 0 var(--rp-shadow);
  --rp-shadow-2: 8px 8px 0 0 var(--rp-shadow);
  /* pressed state: collapse the offset to fake a physical press */
  --rp-shadow-press: 0 0 0 0 var(--rp-shadow);

  /* Blur / elevation — NONE. Depth comes from offset, never blur. */
  --rp-blur: none;

  /* Typography */
  --rp-font: "Press Start 2P", "Pixelify Sans", monospace;
  --rp-size-xs: 8px;
  --rp-size-sm: 10px;
  --rp-size-md: 12px;   /* default UI / labels */
  --rp-size-lg: 16px;   /* sub-headings        */
  --rp-size-xl: 24px;   /* headings            */
  --rp-line: 1.8;       /* generous — pixel fonts need air */
  /* keep bitmap art & fonts crisp; never smooth */
  /* apply on images/canvas/svg: image-rendering: pixelated; */

  /* Spacing scale — 4px base pixel grid (multiples of 4) */
  --rp-space-1: 4px;
  --rp-space-2: 8px;
  --rp-space-3: 12px;
  --rp-space-4: 16px;
  --rp-space-5: 24px;
  --rp-space-6: 32px;
  --rp-space-7: 48px;
}

/* Crisp rendering helpers */
img, canvas, svg.pixel { image-rendering: pixelated; }
svg.pixel * { shape-rendering: crispEdges; }
```
