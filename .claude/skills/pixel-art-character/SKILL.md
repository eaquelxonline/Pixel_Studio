---
name: pixel-art-character
description: Produce game-ready pixel art characters (64–256 px) with the Pixel Studio engine — from an idea, an AI image model's output or a reference — through clean import, 8-direction turnarounds, palette unification, animation and export. Use this whenever the user wants a character, hero, enemy, NPC, sprite, portrait or full-body figure drawn, recreated, cleaned up, rotated to other directions or animated in Pixel Studio, sends an image to turn into pixel art, asks for a prompt for an image model (ChatGPT, Gemini, Midjourney, PixelLab…), or complains that a generated character looks bad — even if they never say "skill" or name a tool.
---

# Pixel art characters with Pixel Studio

## Why this skill exists (read once)

A language model drawing a character from scratch by writing coordinates is working blind: it
makes thousands of pixel decisions with only a handful of looks at the result, sees images at a
coarse resolution, and defaults to safe, average shapes. Research agrees — LLM-written graphics
degrade quickly with complexity (SVGenius, 2025) and render-in-the-loop feedback only partly
closes the gap (Render-in-the-Loop, 2026). Characters made that way here scored 47/100 on our
critic and looked muddy; the same engine turned an image model's concept into an exact 80×110
sprite in 0.6 s.

So the division of labour is:

| Who | Does |
| --- | --- |
| An image model (the user's, or `generate_image`) | The creative concept: design, appeal, pose |
| The engine | Exact measurement and transformation: grid recovery, palette, ramps, splitting, alignment, lighting, AA, export |
| You | Direction, prompts, judgement, the critic loop, fixing details pixel by pixel, animation |

Prefer the hybrid route. Use the from-scratch route only when no image model is available, and
then follow its method exactly (thumbnail first), because that is what makes it work.

## Decide the route

1. Call `image_api_status`.
   - Configured → **API route**: `generate_image` (the user approves each paid call).
   - Not configured → **Prompt route**: give the user a prompt (see below), they generate the image
     and send it; import it with `concept_import`.
   - The user can't or won't use an image model → **From-scratch route**
     (`references/from-scratch.md`).
2. If the user already sent an image, skip to **Import**.

## Prompt route — write the prompt

Fill the template from `references/prompts.md` with a concrete description (age, build, hair,
outfit pieces with colours, props and which hand holds them). Things that matter for the engine:
flat `#FF00FF` background, "clear consistent pixel grid", one light from the top left, ~32
colours, full body with feet visible, a 3/4 view. Explain to the user briefly why (the import
depends on them). Give model-specific tips only for the model they use.

## Import

`concept_import` with the image (payload or `data_base64`; PNG or baseline JPEG; editors convert
other formats). A transparent PNG needs no key colour; on a flat background the border colour is
flood-filled away, and enclosed pockets are removed only for a chroma key (magenta/green), so a
black or white background never eats outlines or highlights. Check the result:

- `cell` is the detected source pixel size and `gridConfidence` its strength. Confidence < 0.2
  usually means the image is not grid-based art (a painting, a blurry upscale): ask for a re-roll
  with the pixel-grid wording, or pass `cell` explicitly if you can measure it.
- `sprite` size should match the character (feet-to-head around 80–120 px for a "128 px" prompt).
- Look at it: `canvas_snapshot` (whole) and zoomed crops (`x, y, width, height`) of the face and
  hands. Compare with the source image side by side when you can.

Then run `art_review`. Imported art keeps the model's style; fix only real defects (stray
background pixels, broken outlines, compression specks the clean-up missed).

`scripts/pipeline.py import <image>` does import + review + exports in one go from a shell.

## Other directions (turnaround)

Eight directions are five unique views (S, SE, E, NE, N); SW, W and NW are mirrors — but mirror
carefully: weapons change hands, asymmetric armour and hair partings must be fixed by hand
afterwards.

1. API route: `generate_image kind=turnaround` (sends the current canvas as reference, imports,
   splits). Prompt route: the turnaround template in `references/prompts.md`; tell the user to
   attach the concept as the reference image. Import the sheet with `concept_import`, then
   `split_sheet names=[S,SE,E,NE,N,NW,W,SW]` (frames, one baseline, centred).
2. Unify colours: every view must use the concept's ramps, or the character flickers between
   directions. `quantize scope=all palette=[concept colours]` (pipeline.py `turnaround
   --palette-from` does this).
3. Check consistency view by view against the concept: height (feet on the same baseline — split
   handles it), proportions, props in the right hand, outfit details present. Redraw what the model
   got wrong with `draw_ascii`/`set_pixels` on zoomed crops; regenerate a single view if it is far
   off.

## Animation

Details and timings: `references/pipeline.md#animation`.

- Idle/breathe, bob, float, sway (capes, hair), hurt flash: `anim_generate` per direction — good
  quality, use freely.
- Attack, cast, hit: duplicate the pose frame, move parts (cut the arm + weapon to its own layer,
  `rotate` with RotSprite around the shoulder), add `fx_generate` slashes/impacts. Keep volumes
  constant; anticipation → fast strike → longer recovery.
- Walk/run: legs must be redrawn per frame. Best: ask the image model for the key poses (contact,
  passing) of each direction with the concept as reference, import them, then do in-betweens and
  cleanup yourself. Without a model, use the from-scratch leg method in `references/pipeline.md`.
- Tag every range (`animation_tag_create`), set durations, preview with `animation_preview`.

## Quality bar

Use `references/quality-bar.md`. In short: `art_review` ≥ 85 with no high-severity issue, value
span ≥ 55 for new art, consistent palette across directions/frames, feet on one baseline, and a
greyscale check (the silhouette and the main light/shadow shapes must read without colour). The
critic finds defects, not beauty — still look at the result yourself, zoomed in.

## Export

`project_export` with `spritesheet` (PNG + JSON), `atlas` (Aseprite JSON, Phaser/PixiJS/Unity),
`godot` (SpriteFrames .tres) or `gif` for previews; `project_save` keeps the editable `.pxs`.

## Talking to the user

Say what each step produced in plain words (sprite size, colours, what was fixed, what is left).
Be honest about weaker parts (e.g. walk cycles without model key poses) and offer the better
route instead of hiding it.
