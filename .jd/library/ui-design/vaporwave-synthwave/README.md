# Vaporwave / Synthwave

## Origin/Era

A 1980s-retrofuturist nostalgia aesthetic that crystallized online in the early-to-mid **2010s**. It remixes the visual language of 1980s consumer tech, anime, early CGI, mall culture, and outrun arcade/cassette art into a hyper-saturated, melancholic-yet-electric "future that never was." Synthwave is the more action-forward, neon-night-drive sibling (Drive, Hotline Miami, Kavinsky); Vaporwave is the slower, more ironic, glitch-and-statue side. In UI they share a palette and toolkit, so they are treated as one reference here.

## Defining traits

- Hot magenta/pink + electric cyan + neon purple on a **deep indigo/near-black** ground.
- **Neon glow** on text, borders, and shapes (layered soft shadows, never a flat color).
- **Sunset gradients**: orange → pink → purple, often behind a glowing geometric sun with horizontal slits.
- **Perspective wireframe grid floor** receding to a horizon (the "outrun" grid).
- Chrome/metallic type, retro digital (segmented/wide) fonts.
- Glitch, RGB-split, and **VHS scanlines**.
- Motifs: palm trees, low sun, dolphins, classical statues, Windows-95-era chrome.
- Mood: nostalgic, dreamy, slightly artificial — high saturation, deep blacks, electric edges.

## When to use

- Music, gaming, streaming, NFT/crypto, and nightlife brands wanting energy and nostalgia.
- Hero sections, splash pages, event/launch landing pages, and "drop" moments.
- Demo/portfolio pieces meant to feel bold and memorable.
- Anywhere a strong emotional, retro-futurist identity beats neutral corporate calm.

## Pitfalls

- **Contrast & accessibility**: neon-on-dark and glow halos wreck legibility; keep body text near-white, reserve glow for accents, and verify contrast.
- **Glow overload**: glow everything and nothing reads as special — pick a few focal elements.
- **Saturation fatigue**: full-bleed neon tires the eye fast; let the dark ground breathe.
- **Performance**: many layered shadows + large blurs + transforms can jank on low-end devices; avoid animating blur.
- **Cliché**: grid + sun + palm tree can feel like a template; lean on typography and layout to stay distinct.
- **Readability of retro fonts**: wide/segmented display fonts are great for headings, terrible for paragraphs — pair with a clean body font.

## Token cheat-sheet

Copy-pasteable design tokens.

```css
:root {
  /* ---- Palette ---- */
  --vw-bg-deep:      #0d0221; /* deep indigo / near-black ground */
  --vw-bg-panel:     #1a0938; /* raised surface (card) */
  --vw-bg-panel-2:   #241046; /* inset / input field */

  --vw-magenta:      #ff2e97; /* hot pink — primary neon */
  --vw-pink-soft:    #ff77c8; /* softer pink for gradients */
  --vw-cyan:         #00f0ff; /* electric cyan — secondary neon */
  --vw-purple:       #b14aed; /* electric purple */
  --vw-violet:       #7b2ff7; /* deep violet accent */

  /* sunset gradient stops */
  --vw-sun-orange:   #ff8a3d;
  --vw-sun-pink:     #ff3c8e;
  --vw-sun-purple:   #9b2fe0;

  --vw-text:         #f5e9ff; /* near-white body text */
  --vw-text-dim:     #b69ad6; /* muted lavender */

  /* ---- Border radius scale ---- */
  --vw-radius-sm:    4px;
  --vw-radius-md:    10px;
  --vw-radius-lg:    18px;
  --vw-radius-pill:  999px;

  /* ---- Spacing scale (4px base) ---- */
  --vw-space-1:      4px;
  --vw-space-2:      8px;
  --vw-space-3:      12px;
  --vw-space-4:      16px;
  --vw-space-5:      24px;
  --vw-space-6:      32px;
  --vw-space-7:      48px;
  --vw-space-8:      64px;

  /* ---- Blur / elevation ---- */
  --vw-blur-sm:      4px;
  --vw-blur-md:      12px;
  --vw-blur-lg:      28px;   /* sun / ambient bloom */
  --vw-elev-1:       0 4px 16px rgba(0,0,0,.45);
  --vw-elev-2:       0 12px 40px rgba(0,0,0,.55);

  /* ---- Typography ---- */
  --vw-font-display: "Orbitron", "Arial Narrow", system-ui, sans-serif; /* headings */
  --vw-font-body:    "Rajdhani", system-ui, -apple-system, sans-serif;  /* body */
  --vw-fw-regular:   400;
  --vw-fw-medium:    600;
  --vw-fw-bold:      700;
  --vw-fw-black:     900;

  --vw-fs-xs:        12px;
  --vw-fs-sm:        14px;
  --vw-fs-md:        16px;
  --vw-fs-lg:        20px;
  --vw-fs-xl:        28px;
  --vw-fs-2xl:       40px;
  --vw-fs-3xl:       64px;  /* hero */
  --vw-tracking:     0.08em; /* wide letter-spacing for display */
}

/* ---- Neon glow recipes (layered shadows) ---- */

/* Text glow — magenta */
.glow-text-magenta {
  color: #fff;
  text-shadow:
    0 0 4px  #ff2e97,
    0 0 10px #ff2e97,
    0 0 24px #ff2e97,
    0 0 48px rgba(255,46,151,.55);
}

/* Text glow — cyan */
.glow-text-cyan {
  color: #eafdff;
  text-shadow:
    0 0 4px  #00f0ff,
    0 0 10px #00f0ff,
    0 0 24px #00f0ff,
    0 0 48px rgba(0,240,255,.5);
}

/* Border/box glow — magenta (inner + outer halo) */
.glow-box-magenta {
  border: 2px solid #ff2e97;
  box-shadow:
    0 0 6px  #ff2e97,
    0 0 18px rgba(255,46,151,.7),
    0 0 40px rgba(255,46,151,.4),
    inset 0 0 12px rgba(255,46,151,.35);
}

/* Border/box glow — cyan */
.glow-box-cyan {
  border: 2px solid #00f0ff;
  box-shadow:
    0 0 6px  #00f0ff,
    0 0 18px rgba(0,240,255,.7),
    0 0 40px rgba(0,240,255,.35),
    inset 0 0 12px rgba(0,240,255,.3);
}

/* ---- Sunset gradient ---- */
.sunset {
  background: linear-gradient(
    180deg,
    #ff8a3d 0%,
    #ff3c8e 45%,
    #9b2fe0 100%
  );
}

/* ---- Perspective grid floor (pure CSS) ---- */
.grid-floor {
  position: absolute;
  inset: 55% 0 0 0;
  overflow: hidden;
  perspective: 280px;
}
.grid-floor::before {
  content: "";
  position: absolute;
  inset: -50% -50% 0 -50%;
  background-image:
    linear-gradient(rgba(0,240,255,.55) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255,46,151,.55) 1px, transparent 1px);
  background-size: 40px 40px;
  transform: rotateX(72deg);
  transform-origin: center top;
}
```
