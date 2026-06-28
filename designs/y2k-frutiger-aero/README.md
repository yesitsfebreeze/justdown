# Y2K / Frutiger Aero

## Origin/Era

Emerged roughly **1999–2010**, spanning the millennial turn through the early
smartphone era. Born from turn-of-the-century techno-optimism: clean energy,
the "wet" web, and consumer hardware finally powerful enough to render
gradients, soft shadows, and translucency in real time. The aesthetic was
codified by **Windows XP/Vista Aero**, **Mac OS X Aqua**, the **iPod/iTunes**
era, and corporate stock photography full of water droplets, bubbles, blue
skies, and lush green grass. Named retroactively after the **Frutiger**
typeface family (Adrian Frutiger) that defined its humanist, friendly signage.
The mood is glossy, clean, hopeful, eco-tech utopian.

## Defining traits

- Aqua and sky-blue gradients; cyan-to-white "atmospheric" backgrounds.
- Glossy, skeuomorphic **glass buttons** with a bright specular highlight
  across the top half (the "Aqua pill" / candy-gel look).
- **Aero glass** translucency: frosted, blurred panels you can see through.
- Water imagery — droplets, bubbles, ripples, condensation.
- Lush nature motifs: green grass, blue sky, fish, leaves, clouds.
- Everything **rounded** — generous corner radii on cards, buttons, inputs.
- Specular sheen, lens flare, and soft bloom highlights.
- Soft, diffuse drop shadows (never hard or graphic).
- Humanist sans typography (Frutiger / Myriad / Segoe-like), clean and warm.
- Reflective surfaces and subtle "wet" reflections beneath elements.

## When to use

- Nostalgic / retro-tech products evoking 2000s computing.
- Eco, clean-energy, wellness, or "fresh & clean" brand stories.
- Playful consumer apps wanting an optimistic, approachable feel.
- Music, media, and lifestyle UIs referencing the iPod/iTunes era.
- Landing pages or event sites that want an instantly recognizable vibe.

## Pitfalls

- Over-glossing **everything** kills hierarchy — reserve the heaviest sheen
  for primary actions.
- Heavy `backdrop-filter` blur is expensive; can tank performance on long
  lists or low-end devices.
- Low contrast: glossy gradients + thin light text often fail WCAG; check
  text-on-glass legibility.
- Skeuomorphic clutter dates fast — keep layouts modern even if surfaces glossy.
- Too many competing gradients turn the page muddy; anchor with whites/cyans.
- Bevels + inner shadows + outer shadows stacked look noisy; pick a recipe.

## Token cheat-sheet

### Palette

```
--aero-sky-top:     #aee4ff;  /* pale cyan sky                */
--aero-sky-bottom:  #4ab8ef;  /* saturated aqua               */
--aero-deep:        #0d6fb8;  /* deep ocean blue (text/accent)*/
--aero-aqua:        #19c2ff;  /* candy aqua (button base)     */
--aero-aqua-dark:   #0a8fd6;  /* aqua shadow stop             */
--aero-green:       #7ed957;  /* fresh grass green            */
--aero-green-dark:  #3fa516;  /* deep leaf green              */
--aero-glass:       rgba(255,255,255,0.35); /* frosted fill   */
--aero-glass-line:  rgba(255,255,255,0.65); /* glass edge     */
--aero-ink:         #0a3c5c;  /* primary text                 */
--aero-ink-soft:    #3a6b88;  /* secondary text               */
--aero-white:       #ffffff;
```

### Border-radius scale

```
--r-xs:  6px;   /* inputs, chips        */
--r-sm:  12px;  /* small controls       */
--r-md:  18px;  /* buttons              */
--r-lg:  28px;  /* cards / panels       */
--r-pill: 999px;/* aqua pill buttons    */
```

### Shadow recipes

```
/* soft card elevation */
--shadow-card:
  0 18px 40px -12px rgba(13,111,184,0.45),
  0 2px 6px rgba(10,60,92,0.18);

/* glossy button (outer lift + inner top sheen + inner bottom core) */
--shadow-button:
  0 8px 18px -4px rgba(10,143,214,0.55),
  inset 0 1px 0 rgba(255,255,255,0.95),
  inset 0 -10px 16px -8px rgba(10,143,214,0.65);

/* frosted glass edge */
--shadow-glass:
  inset 0 1px 0 rgba(255,255,255,0.7),
  inset 0 0 0 1px rgba(255,255,255,0.25),
  0 10px 30px -8px rgba(13,111,184,0.4);
```

### Blur / elevation

```
--blur-glass:  14px;   /* backdrop-filter for Aero panels   */
--blur-bloom:  40px;   /* soft highlight bloom / lens flare */
--glass-alpha: 0.35;   /* panel fill opacity                */
```

### Typography

```
font-family: "Inter", "Segoe UI", "Myriad Pro", "Frutiger",
             system-ui, sans-serif;

--w-regular: 400;
--w-medium:  500;
--w-bold:    700;

--fs-display: 32px;  /* hero/header title  */
--fs-h2:      22px;  /* card title         */
--fs-body:    15px;  /* body / labels      */
--fs-small:   13px;  /* hints / captions   */

letter-spacing (titles): -0.01em;
line-height (body): 1.5;
```

### Spacing scale (8pt base)

```
--sp-1: 4px;
--sp-2: 8px;
--sp-3: 12px;
--sp-4: 16px;
--sp-5: 24px;
--sp-6: 32px;
--sp-7: 48px;
```
