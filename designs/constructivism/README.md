# Constructivism

## Origin/Era

Russian/Soviet Constructivism (~1915–1930s), born out of the revolution and tied to artists like **Alexander Rodchenko**, **El Lissitzky**, and **Varvara Stepanova**. It rejected "art for art's sake" in favor of art in service of social and industrial purpose — design as a tool for building a new society. Its most visible output was the **agitprop poster**, weaponizing typography and photomontage for mass communication. The defining image is El Lissitzky's 1919 poster *"Beat the Whites with the Red Wedge"*, a stark geometric metaphor for revolutionary force.

## Defining traits

- Bold diagonals and dynamic angled compositions
- Heavy geometric sans / stencil type, often set on angles
- Red + black + cream/off-white palette
- Photomontage and collage
- Strong directional energy that drives the eye
- Thick rules and bars as structural elements
- Circular and wedge motifs as recurring shapes
- Revolutionary, industrial, machine-age feel

## When to use

- Activist, political, or movement-driven messaging
- Bold editorial covers, posters, and event branding
- Brands wanting an austere, industrial, anti-decorative voice
- Manifestos, declarations, and high-impact landing pages
- Music, art, and culture sites that want raw geometric energy
- Anywhere a loud, confrontational, propaganda-poster tone fits

## Pitfalls

- Diagonals everywhere harm legibility and scannability of real UI
- Red + black at full strength fatigues the eye and reads as alarm
- The aesthetic carries heavy political/historical baggage — wrong fit for neutral corporate or financial products
- Hard to make accessible: low-contrast cream-on-red, rotated text fails screen readers and reflow
- Easy to slip into pastiche/cliché — a red wedge alone is not Constructivism
- Cramped data-dense interfaces (tables, dashboards) fight the angular layout

## Token cheat-sheet

**Palette**
- Constructivist red: `#C81910`
- Black: `#0E0E0E`
- Cream / paper: `#ECE6D6`
- Muted gold (accent, optional): `#C9A227`

**Border-radius**
- `0` everywhere — flat hard edges
- Exception: wedges and full circles as deliberate accents (`border-radius: 50%` for circular motifs, `clip-path` for wedges)

**Shadow / elevation**
- Flat, hard offset shadows only: `box-shadow: 8px 8px 0 #0E0E0E`
- No soft shadows, no blur, no glow
- Blur: `none`. Elevation: implied by overlap and hard offset, never by softness

**Typography**
- Heavy condensed/geometric sans: Anton, Oswald, or a stencil face
- Uppercase, tight tracking, large weight contrast
- Headlines often set on diagonals (`transform: rotate(-6deg)`)

**Spacing**
- 8px base unit
- Dynamic, asymmetric layout — diagonal axes, intentional imbalance
- Bars and rules used as spacing/structural devices, not just text
