# Quality bar

The critic (`art_review`) measures defects: value range, banding, orphan pixels, jaggies,
pillow shading, outline coverage, contrast with the background. It cannot tell an appealing
design from a dull one. Use both: numbers for defects, your eyes (and the user's) for appeal.

## Numbers

| Check | Target | Notes |
| --- | --- | --- |
| `score` | ≥ 85 | and no `high` severity issue |
| `values.span` | ≥ 55 for new art | imported concepts: accept the model's look unless it reads muddy |
| `banding.per100` | ≤ 1 | `fix_banding` |
| `orphansPercent` | ≤ 1.5 | isolated single pixels; some are intended highlights |
| `pillow` | empty | light must come from one side, not from the centre outwards |
| `jaggies` | ≤ 3 on a 128 sprite | `antialias`, or redraw the curve with `draw_path` |
| colours | ≤ 32 per character | shared across all directions and frames |

## Look-test routine (do it every time)

1. Whole sprite at 2–4× (`canvas_snapshot scale=4`).
2. Zoomed crops of the head, hands and feet (`canvas_snapshot x/y/width/height`) — most errors
   hide there: eyes merged into skin, fingers as blobs, feet floating.
3. The greyscale panel of `art_review`'s contact sheet: the silhouette must read, and the head
   must be the brightest or highest-contrast area.
4. Thumbnail: at 1× the pose and the props must still be recognisable.
5. Directions side by side (`split_sheet into=layers` or the frame strip): same height, same
   baseline, same colours, prop in the right hand.
6. Animations: preview each tag; look for volume changes (the body shrinking), jitter of
   pixels that should stay still, and loops that pop on the last frame.

## Common failures and what they mean

- Muddy/flat: ramps too short or the base tones in the middle of the ramp for every material —
  plan values first (dark hair, mid tunic, light skin/metal).
- "Pillow" shading: shading from the outline inwards instead of from one light.
- Noisy: too many colours or dithering at small sizes; quantize to the palette.
- Stiff pose: both feet parallel, arms straight down. Ask for "weight on one leg", tilt the
  shoulders against the hips (contrapposto).
- Floating: the feet don't touch the baseline after split or animation.
