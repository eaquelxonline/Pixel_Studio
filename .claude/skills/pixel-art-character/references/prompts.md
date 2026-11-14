# Image-model prompts for Pixel Studio

The engine's `concept_import` recovers the pixel grid, keys out the background and builds ramps.
It works best when the image is *already* grid-based pixel art on a flat key colour. Every
template below keeps those parts — don't drop them when you personalise the prompt.

The same templates live in code (`ConceptPrompts` in `Gateway/Source/Main/Kotlin/Core.kt`) and
are served as the MCP prompts `concept_prompt` / `turnaround_prompt`; keep the two in sync.

## Contents
- Describing the character
- Concept (one view)
- Turnaround (8 directions)
- Key poses for animation
- Props, portraits, effects
- Negative prompt
- Model notes
- Re-roll checklist

## Describing the character

Write a concrete, visual description; image models follow nouns and colours, not adjectives
about quality. Cover, in this order:

1. Who: role, age range, build, height impression ("lean young ranger, tall").
2. Head: hair shape and colour, face features that read at 100 px (eye colour rarely does; scars,
   beards, masks, glowing eyes do).
3. Outfit, top to bottom, each piece with a colour: hood, cloak, tunic, belt, trousers, boots.
4. Props and *which hand* holds them (this matters for the turnaround and mirroring).
5. Mood/pose: "relaxed idle stance, weight on one leg".

Keep designs original. If the user asks for a known franchise character, offer an original
character in a similar style instead of an exact copy.

## Concept (one view)

```
Pixel art game sprite of {CHARACTER}. Full body, standing idle pose, {VIEW} view (3/4 front),
feet visible, centred. Character about {SIZE} pixels tall on a {CANVAS}x{CANVAS} canvas,
clear consistent pixel grid with every pixel the same size, crisp hard pixel edges, no
anti-aliasing blur, limited palette of about 32 colours, clean 1-pixel dark outline,
one light source from the top left with clear light and shadow shapes, high contrast,
readable silhouette. Flat solid #FF00FF magenta background, no ground shadow, no scenery,
no text, no border.
```

- `{SIZE}`: 96–120 for a 128 canvas; 48–60 for 64. Bigger sprites keep more detail but cost
  more animation work.
- `{VIEW}`: "south-east" (3/4 front) is the most useful starting view; "south" for top-down games.
- If the character has magenta in the design, use `#00FF00` green as key and say so everywhere.

## Turnaround (8 directions)

Attach the concept image as reference. Asking for all eight views in one image keeps proportions
and colours consistent across views — much better than eight separate generations.

```
Using the attached character as the exact reference (same design, colours, proportions and
props), draw a pixel art turnaround sheet: 8 views in one horizontal row, left to right facing
south, south-east, east, north-east, north, north-west, west, south-west. Same height and same
ground line for every view, equal spacing with clear empty gaps between views, same idle pose.
Clear consistent pixel grid, crisp hard pixels, about 32 colours, 1-pixel dark outline, light
from the top left in every view. Flat solid #FF00FF magenta background, no text, no labels.
```

If the model squeezes eight views badly, ask for the five unique ones (S, SE, E, NE, N) and
mirror the rest in the engine (see pipeline.md — mirrored views need hand fixes for asymmetric
details and the weapon hand).

## Key poses for animation

Use for walk/run/attack where legs or big body motion must be redrawn. One direction per request,
concept as reference:

```
Using the attached character as the exact reference, draw a pixel art animation key-pose sheet
for a {ACTION} facing {DIRECTION}: {N} poses in one horizontal row with clear gaps:
{POSES}. Identical character design, size, colours and ground line in every pose. Clear
consistent pixel grid, crisp pixels, 1-pixel dark outline, light from the top left. Flat solid
#FF00FF magenta background, no text, no motion lines, no effects.
```

Typical `{POSES}`:
- walk (4): "contact (right foot forward), passing, contact (left foot forward), passing"
- run (4): "contact, down, push-off, airborne"
- attack (3): "anticipation wind-up, strike at full extension, follow-through"
- cast (3): "gather (hands low), raise, release (arms forward)"
- hurt/death: "hit recoil", "kneel", "fallen"

Import with `concept_import`, then `split_sheet` into frames; the in-betweens are drawn in the
engine.

## Props, portraits, effects

- Prop / weapon icon: "Pixel art game item icon of {ITEM}, 32x32 pixels, centred, clear pixel
  grid, 1-pixel dark outline, light from top left, flat #FF00FF background."
- Portrait: "Pixel art character portrait bust of {CHARACTER}, head and shoulders, 96x96
  pixels, 3/4 view, …(same grid/outline/background wording)".
- Effects (fire, auras, slashes) are better made procedurally with `fx_generate` /
  `fx_particles` than with an image model: they loop cleanly and tile across frames.

## Negative prompt

For models that take one (Stable Diffusion family, some APIs):

```
blurry, soft edges, anti-aliasing, gradients, noise, jpeg artifacts, painterly, 3d render,
photo, mixed pixel sizes, sub-pixel detail, dithering noise, text, watermark, signature,
border, frame, scenery, ground shadow, cropped feet, multiple characters
```

## Model notes

These change often; check `image_api_status` and the provider's docs rather than trusting a
remembered model name.

- **OpenAI image models (ChatGPT / API)**: follow long instructions well; ask for "1024x1024"
  and the grid wording; they tend to add soft shading — "hard pixels, no gradients" helps.
- **Gemini image models**: good at editing with a reference (turnarounds, key poses); repeat
  "same design as the reference" and the background colour.
- **Midjourney**: add `--style raw`; avoid `--stylize` high values; it often breaks the grid,
  so expect lower `gridConfidence`.
- **PixelLab / Retro Diffusion**: pixel-art specialised, produce true grids at native size —
  ideal input. PixelLab also has rotation/animation endpoints; their output imports directly.

## Re-roll checklist

Ask the user (or `generate_image` again) for a new image when:

- `gridConfidence` < 0.2 and you can't measure the cell yourself;
- the background is a gradient or has scenery (the key-out fails);
- feet or the top of the head are cropped;
- the character holds the prop in the other hand than the brief says (it breaks the turnaround);
- the design is wrong in a way that would take more than a few minutes to repaint.
