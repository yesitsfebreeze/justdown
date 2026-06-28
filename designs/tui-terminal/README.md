# TUI / Terminal

A CSS rendering of a text-UI / terminal aesthetic: monospace everything, box-drawing characters (┌─┐│└┘├┤), block cursors, ASCII/ANSI color, phosphor green or amber on near-black, optional CRT scanlines + subtle glow, no rounded corners, character-grid alignment. It must still be a REAL HTML page — not an image — styled to look like a terminal.

## Origin/Era

Rooted in the text terminals of the 1970s–1990s: the DEC VT52/VT100 (1978), IBM 3270 mainframe terminals, and the monochrome phosphor CRTs (P1 green, P3 amber) that defined how computing *looked* before the GUI. The visual grammar — box-drawing glyphs from code page 437 / DOS, ncurses TUIs, ANSI escape colors, the steady block cursor — comes from this era. The modern revival (90s–2000s onward) lives in tools like `htop`, `vim`, `tmux`, `midnight commander`, and the 16-color ANSI palettes shipped by terminal emulators. As a *design style* it signals: low-level, hacker, retro-computing, "the machine speaking plainly."

## Defining traits

- **Monospace, always.** Every character occupies one cell; the whole layout aligns to a character grid.
- **Box-drawing frames.** Windows and cards are framed with `┌ ─ ┐ │ └ ┘ ├ ┤ ┬ ┴ ┼` (or double-line `╔ ═ ╗ ║`), not CSS rounded rectangles.
- **Phosphor palette.** Near-black background; foreground in P1 green (`#33ff33`) or P3 amber (`#ffb000`), or a full 16-color ANSI set for "color" UIs.
- **Block cursor.** A solid `█` (or reverse-video block) that blinks; selection and focus are reverse-video, not drop shadows.
- **CRT artifacts (optional).** Horizontal scanlines via `repeating-linear-gradient`, soft phosphor glow via `text-shadow`, sometimes a faint vignette/curvature.
- **No rounded corners. No gradients-as-decoration. No soft shadows.** Elevation is faked with characters and reverse video, not blur.
- **Buttons read as labels:** `[ OK ]`, `[ Cancel ]`, `< Submit >`.
- **Inputs read as prompt lines:** `> _` with a trailing block cursor.

## When to use

- Developer tools, CLIs-with-a-face, status dashboards, log viewers, deployment/CI surfaces.
- Hacker / retro-computing / cyberpunk brand moments; landing pages that want "we are close to the metal."
- Game UIs (roguelikes, sci-fi consoles) and "fake OS" interactive fiction.
- Anywhere the *content is text-dense and tabular* and you want density + legibility over decoration.
- Onboarding/easter-egg surfaces where a wink at retro computing builds rapport.

## Pitfalls

- **Legibility vs. effect.** Heavy scanlines + heavy glow + low contrast = unreadable. Keep glow subtle and contrast high; offer a "reduce effects" path.
- **Box-drawing alignment breaks** the instant the font is not truly monospace, or `line-height` ≠ the cell height, or letter-spacing is nonzero. Frames will visibly tear. Lock the font stack and line-height.
- **Accessibility.** Pure green-on-black can fail contrast for some users and is rough for color-blind/low-vision; respect `prefers-reduced-motion` (kill blink/scanline animation) and provide adequate contrast ratios.
- **Emoji / non-mono glyphs** inside a grid break the column math. Stick to ASCII + box-drawing.
- **Overuse.** The aesthetic is loud; it fights long-form reading and dense forms. Reserve it for surfaces where the vibe earns its keep.
- **Don't fake it with an image.** It must be real, selectable, responsive text.

## Token cheat-sheet

Copy-pasteable CSS custom properties.

```css
:root {
  /* ---- Palette: classic green phosphor (P1) ---- */
  --term-bg:            #0b0f0a;  /* near-black, faint green tint */
  --term-bg-raised:     #0f150e;  /* "raised" panel, one notch lighter */
  --green-phosphor:     #33ff33;  /* primary foreground (P1) */
  --green-dim:          #1f9d1f;  /* secondary / muted text */
  --green-bright:       #b6ffb6;  /* highlight / bold */

  /* ---- Palette: amber phosphor (P3) — swap in for amber builds ---- */
  --amber-phosphor:     #ffb000;  /* primary foreground (P3) */
  --amber-dim:          #a86b00;  /* secondary / muted */
  --amber-bright:       #ffd277;  /* highlight / bold */

  /* ---- Palette: modern 16-color ANSI terminal set ---- */
  --ansi-black:         #1c1c1c;  /* 0  black            */
  --ansi-red:           #ff5f56;  /* 1  red             */
  --ansi-green:         #5af78e;  /* 2  green           */
  --ansi-yellow:        #f3f99d;  /* 3  yellow          */
  --ansi-blue:          #57c7ff;  /* 4  blue            */
  --ansi-magenta:       #ff6ac1;  /* 5  magenta         */
  --ansi-cyan:          #9aedfe;  /* 6  cyan            */
  --ansi-white:         #d0d0d0;  /* 7  white           */
  --ansi-br-black:      #686868;  /* 8  bright black    */
  --ansi-br-red:        #ff6e67;  /* 9  bright red      */
  --ansi-br-green:      #5af78e;  /* 10 bright green    */
  --ansi-br-yellow:     #f4f99d;  /* 11 bright yellow   */
  --ansi-br-blue:       #57c7ff;  /* 12 bright blue     */
  --ansi-br-magenta:    #ff6ac1;  /* 13 bright magenta  */
  --ansi-br-cyan:       #9aedfe;  /* 14 bright cyan     */
  --ansi-br-white:      #ffffff;  /* 15 bright white    */

  /* ---- Border radius: terminals don't round ---- */
  --radius: 0;

  /* ---- Shadow recipe: CRT phosphor glow (NOT a drop shadow) ---- */
  /* foreground glow — apply to text, scaled by intensity */
  --glow-soft:   0 0 2px currentColor;
  --glow:        0 0 4px currentColor, 0 0 8px currentColor;
  --glow-strong: 0 0 4px currentColor, 0 0 11px currentColor, 0 0 19px currentColor;
  --box-shadow:  none;            /* panels cast no shadow */

  /* ---- Blur / elevation: scanlines + screen glow, no z-blur ---- */
  /* scanline overlay — use as a ::after on a full-screen layer */
  --scanline: repeating-linear-gradient(
                to bottom,
                rgba(0,0,0,0)   0px,
                rgba(0,0,0,0)   2px,
                rgba(0,0,0,.28) 3px,
                rgba(0,0,0,.28) 3px
              );
  --scanline-opacity: 0.5;
  --screen-vignette: radial-gradient(
                       ellipse at center,
                       rgba(0,0,0,0) 60%,
                       rgba(0,0,0,.45) 100%
                     );
  --flicker-anim: flicker 6s infinite steps(60); /* very subtle, optional */

  /* ---- Typography: true monospace, locked to the cell grid ---- */
  --font-mono: "IBM Plex Mono", "JetBrains Mono", "VT323",
               "DejaVu Sans Mono", "Menlo", "Consolas", monospace;
  --fs-xs:   12px;
  --fs-sm:   14px;
  --fs-base: 16px;   /* one cell ≈ 16px tall */
  --fs-lg:   20px;
  --fs-xl:   28px;   /* banners / ASCII headers */
  --lh:      1.2;    /* MUST keep cells square-ish; do not vary per element */
  --letter-spacing: 0;          /* nonzero spacing tears box-drawing frames */
  --font-weight-normal: 400;
  --font-weight-bold:   700;

  /* ---- Spacing: character-grid based (1ch ≈ one column) ---- */
  --cell-w: 1ch;     /* one character column */
  --cell-h: calc(var(--fs-base) * var(--lh)); /* one row */
  --space-1: 1ch;
  --space-2: 2ch;
  --space-3: 3ch;
  --space-4: 4ch;
  --gutter:  2ch;    /* default inner padding for framed boxes */
}

/* Scanline overlay layer (full-screen, pointer-events:none) */
.crt-scanlines::after {
  content: "";
  position: fixed; inset: 0;
  background: var(--scanline);
  opacity: var(--scanline-opacity);
  pointer-events: none;
  z-index: 9999;
}

/* Respect reduced motion: kill blink + flicker */
@media (prefers-reduced-motion: reduce) {
  * { animation: none !important; }
}
```
