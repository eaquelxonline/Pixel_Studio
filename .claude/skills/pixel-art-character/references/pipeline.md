# Pipeline: exact tool sequences

Argument names are the engine's (the same in MCP, Anthropic, OpenAI and Gemini schemas). Read the
result of each step before the next one; every mutating call returns `ok`, a revision and
step-specific fields.

## Contents
- 1. Import a concept
- 2. Clean-up
- 3. Turnaround
- 4. Palette unification
- 5. Mirroring
- 6. Animation
- 7. Tags, timing, preview
- 8. Export

## 1. Import a concept

```json
{"op":"project_new","width":128,"height":128,"name":"Ranger"}
{"op":"concept_import","data_base64":"<PNG or JPEG>","colors":32}
```

Optional arguments: `cell` (force the source pixel size), `sample` (0.5 = centre of each cell),
`background` (`auto` keys out the border colour, `keep` keeps it), `background_tolerance` (0.1),
`clean` (0.07; removes isolated compression specks), `margin` (2), `fit_canvas` (true: the canvas
is resized to the sprite + margin), `ramps` (true: build named ramps from the colours), `layer`.

Result: `cell`, `gridConfidence`, `sprite {width,height}`, `colors`, `ramps`, `backgroundPixels`,
`cleanedPixels`.

- `cell` 1 with a low confidence on a native-size sprite (e.g. PixelLab output) is fine — it was
  already at 1 px per pixel.
- With the API route, `generate_image` does this step for you and saves the raw image to Exports.

## 2. Clean-up

Look first: `canvas_snapshot` and zoomed crops, e.g. `{"op":"canvas_snapshot","x":28,"y":4,
"width":32,"height":32}` for the head. Then `art_review` (scale 4 gives a contact sheet with the
greyscale view).

Fix only real defects; imported art keeps the model's style.

| Problem | Fix |
| --- | --- |
| Background specks left around the outline | `set_pixels` with `"color":"#00000000"`, or `pixel_cleanup` |
| Banding (parallel staircase lines) | `fix_banding` on the layer |
| Jagged curves | `antialias amount=0.5` (uses existing ramp colours) |
| Flat / muddy values (span < 55) | widen the ramps: `gradient_map` or re-light with `shade_form` on that material; for imported art prefer small `palette_swap` changes of the darkest/lightest tones |
| Broken outline | `outline mode=solid` on a silhouette copy, or redraw with `draw_path` |
| Face unreadable | redraw eyes/mouth with `draw_ascii` on a zoomed crop: 1–2 px eyes, a dark brow line, a light cheek pixel |

## 3. Turnaround

API route:

```json
{"op":"generate_image","kind":"turnaround","character":"<same description as the concept>"}
```

It sends the current canvas as reference, imports the sheet and splits it into eight frames
named S…SW. Approval is required (paid call).

Prompt route: give the user the turnaround prompt (prompts.md), import their image, then:

```json
{"op":"concept_import","data_base64":"<sheet>","fit_canvas":false}
{"op":"split_sheet","into":"frames","names":["S","SE","E","NE","N","NW","W","SW"]}
```

`split_sheet` finds sprites by empty columns (`gap` 2 by default), drops specks, centres each
sprite in a common canvas and stands them on one baseline. With five views, pass five names and
mirror the rest (section 5). `into:"layers"` is handy to compare views side by side.

If the model drew views touching each other, split fails to separate them: ask for "clear empty
gaps between views", or cut them yourself with `move_region` / `copy_region`.

## 4. Palette unification

Each generated view comes with slightly different colours; frames that should share a palette
flicker otherwise. Map everything onto the concept's colours:

```json
{"op":"quantize","scope":"all","palette":["#1b1424","#3a2b4a", "..."]}
```

`scripts/pipeline.py turnaround --palette-from concept.png` extracts the concept colours and
does this. Then re-check faces: quantising can merge a 1-px eye into the skin. Use
`palette_swap` for deliberate colour changes (a variant enemy, team colours).

## 5. Mirroring

To make W, NW, SW from E, NE, SE:

```json
{"op":"frame_duplicate","frame":2}
{"op":"flip","axis":"horizontal","frame":3}
```

Then fix, per mirrored view:

- weapon / shield hand (swap back by moving the prop layer or redrawing);
- asymmetric details: hair parting, a single pauldron, scars, belt buckle off-centre, quiver;
- the light: the engine lights from the top left, a mirrored sprite is lit from the top right.
  Re-light big masses with `shade_form light=top-left` or swap the light/shadow tones on large
  areas with `palette_swap` limited to the layer. Small details can stay.

## 6. Animation

Cheap and good (procedural, per direction frame):

```json
{"op":"anim_generate","kind":"breathe","frames":6,"frame_ms":140,"frame":0,"tag":"idle_S"}
```

`kind`: `breathe` (idle chest/shoulders), `bob`, `float` (hovering), `sway` (cloaks, hair,
plants), `squash` (slimes, jumps), `shake`, `pulse` (glows), `hurt` (white flash + knock-back).
`amount` scales the motion; keep it small (≤ 1.0) for idle.

Part-based motion (attack, cast, nod): split the moving part into its own layer first —
copy the region of the arm + weapon to a new layer (`layer_create` + `copy_region`), clear it on
the body, then per frame:

```json
{"op":"frame_duplicate","frame":0}
{"op":"rotate","layer":"Arm","frame":1,"degrees":-35,"method":"rotsprite","center":[52,40]}
{"op":"fx_generate", "...": "slash / impact on its own layer"}
```

Timing for a sword swing at ~10 fps: anticipation 2 frames × 120 ms, strike 1–2 frames × 50 ms,
follow-through/recovery 3 frames × 100 ms. Big rotations (> 45°) look better redrawn than
rotated; RotSprite keeps pixel clusters but thins 1-px lines.

Walk/run (legs redrawn every frame):

1. Best: key poses from the image model (prompts.md → "Key poses"), imported and split into
   frames; draw the in-betweens by hand and keep the palette (quantize to the concept palette).
2. Without a model: draw the legs on their own layer with `draw_path` strokes per frame, using
   the four poses contact → passing → contact (other leg) → passing. Keep the head/torso bobbing
   1 px down on contact and 1 px up on passing. 8 frames × 80–100 ms for a walk, 6 × 70 ms for
   a run. Expect to iterate with zoomed snapshots; legs read as "stilts" until knees bend.

Hurt and death: `anim_generate kind=hurt` for the flash; a death is 3–4 drawn poses (recoil,
kneel, fall, lie) plus `fx_particles` dust.

## 7. Tags, timing, preview

```json
{"op":"animation_tag_create","name":"walk_E","from":8,"to":15,"direction":"forward"}
{"op":"frame_set_duration","frame":8,"duration":90}
{"op":"animation_preview","tag":"walk_E","scale":3}
```

Name tags `<action>_<direction>` (idle_S, walk_NE …) — engines and the atlas exporter group by
tag name.

## 8. Export

```json
{"op":"project_export","format":"spritesheet","scale":1}
{"op":"project_export","format":"atlas"}
{"op":"project_export","format":"godot","name":"ranger"}
{"op":"project_export","format":"gif","tag":"idle_S","scale":3}
{"op":"project_save"}
```

`atlas` is Aseprite-compatible JSON (Phaser, PixiJS, Unity importers); `godot` writes a
SpriteFrames `.tres`; `trim`, `padding`, `extrude` and `columns` control the sheet layout.
