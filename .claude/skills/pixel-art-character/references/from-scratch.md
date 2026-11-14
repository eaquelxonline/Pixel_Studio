# From scratch, without an image model

Use only when the user can't or won't use an image model. Set expectations: the result will be
clean and consistent, but less inventive than a model's concept.

## Why the usual approach fails

Drawing a 128 px character directly means placing ~8 000 pixels while seeing the result only a
few times, at reduced resolution. Errors in proportion and pose get locked in early and the
final pass can only polish them. Research on LLM-generated graphics shows the same: quality
drops with complexity, and looking at the render after every step (render-in-the-loop) is what
helps most.

The fix is the method artists use: **decide the big shapes at a small size, then scale and
refine.** A 32–48 px thumbnail has few enough pixels to reason about each one, and each look at
the snapshot covers the whole design.

## Method

1. **Brief**: write the design in words (role, silhouette idea, 3–4 materials with colours,
   value plan: which material is darkest/lightest).
2. **Ramps**: `ramp_set` one ramp per material, 4–6 tones from a real dark to a real light,
   hue-shifted (shadows cooler, lights warmer).
3. **Thumbnail** at 1/4 of the final size (32×32 for a 128 canvas, 48×48 for a 192 one) with
   `draw_ascii` — one character per pixel, a legend per tone. Draw the silhouette first, then the
   big light/shadow shapes. Snapshot after every change at scale 8–12. Iterate until the pose,
   proportions and silhouette are right; this is the step that decides the quality.
4. **Upscale**: `{"op":"upscale","factor":4,"method":"epx"}` — EPX doubles twice and rounds the
   stairs instead of making blocky 4×4 pixels.
5. **Refine** at full size, one area at a time with zoomed snapshots:
   - outlines: one dark silhouette outline (`outline mode=solid` on a silhouette layer), inner
     lines in the ramp's dark tone, not black;
   - forms: `shade_form` per material from one light (top-left);
   - details: face, hands, buckles, folds with `draw_path` and `set_pixels`;
   - finish: `fix_banding`, `antialias amount=0.5`.
6. **Critic loop**: `art_review` → fix the highest-severity issue → repeat until the bar in
   quality-bar.md is met. Stop and show the user when the remaining issues are taste, not
   defects.

`draw_figure` gives a posed, proportioned mannequin (heads, pose, facing, build) to dress with
`draw_path` shapes; it is a good alternative to step 3 for human figures. See
`Assets/Sprites.py` (target `sela`) for a complete example built this way.

## Thumbnail example (32 px, top of a ranger)

```json
{"op":"draw_ascii","x":12,"y":2,"legend":{"o":"#14101c","h":"#3b5d3a","H":"#5e8a4e","s":"#d79a6e","S":"#f2c49a","e":"#14101c"},
 "rows":["  oooo  ",
         " ohHHho ",
         "ohHHHHho",
         "ohsssSho",
         "ohesseho",
         " osssso ",
         "  oooo  "]}
```

Hood shapes first, then the face window with a lit cheek (`S`) and two eye pixels; spaces are
transparent.

## What worked best (Lantern Warden, `Assets/Sprites.py warden`)

- Flat `draw_path` shapes per part, each also drawn into a `Sil` layer that gets one solid dark
  outline at the end; selective `inside` outlines separate the parts.
- Hand-drawn `draw_ascii` at 1:1 for the dense areas: face (≈17×20), bangs, hands (8×8) and the
  key prop. This gave the largest jump in quality.
- `shade_form` for volume, then hand-placed shadow polygons (`shade` with pixel lists): cast
  shadows under hair and belt, cloth folds, knee and ankle creases.
- A warm rim on the edges facing the key light and a cool back-rim on the other side.
- `quantize` each layer to the ramps (+ a few accent colours) at the end.
- Compare side by side with a strong reference (an imported concept) to catch proportion
  problems: here, short legs and a narrow stance.
- For animation, rebuild every frame from the same parametric description (breath offset, sway
  phase, blink, flicker) instead of shifting layers: every frame gets its own lighting and
  outlines, so there are no seams.

## Iteration discipline

- One area per step; snapshot after each.
- Compare with the brief, not with the last version.
- If after three passes an area still reads wrong, redraw it from the thumbnail instead of
  patching.
- Save versions (`project_save`) before big changes.
