package com.pixel.studio.gateway.prompts

import com.pixel.studio.gateway.jsonrpc.JsonRpc
import com.pixel.studio.gateway.jsonrpc.RpcException

/** Text that teaches any connected AI how to work as a pixel artist in Pixel Studio. */
object Guidance {
    val SERVER_INSTRUCTIONS = """
        Pixel Studio is a shared pixel-art and animation workspace. A human may be drawing on the same canvas
        while you work. You draw with real pixel tools; nothing is generated for you.

        Workflow:
        1. canvas_inspect → learn size, layers, frames and the current revision.
        2. Plan a small palette (palette_add) and split the subject into layers (layer_create: Shadow, Body, Head, ...).
        3. Block in shapes (draw_rectangle/draw_ellipse/fill_region), then refine with set_pixels.
           Batch related edits into one set_pixels or transaction call — each call is one undo step.
        4. Check your work with canvas_snapshot (image) or read_region (ascii text) after each major step.
        5. Animation: frame_duplicate or copy_region the previous pose, then change only what moves.
           Set timing with frame_set_duration, group ranges with animation_tag_create, review with animation_preview.

        3D pixel art (voxels): voxel_model_create → texture_generate/material_set (PBR) → rig_bone_add → voxel_shape /
        voxel_set_many → rig_bind → anim3d_generate (walk/run/idle) → voxel_render to look at it (shadows, bloom, lights) →
        voxel_render_sheet or voxel_render_to_frames with directions=8 for game sprites (also pass="normal" for normal maps).
        Chunky models (40-80 voxels tall) read best as sprites. VFX: build a base once, voxel_model_clone it per frame,
        add the moving voxels (flames, sparks, smoke with alpha) and render the list with voxel_render_sheet models=[...].

        Collaboration rules:
        - Pass expectedRevision when you edit after reading; on a 'conflict' error re-inspect and retry — never overwrite blindly.
        - Use operationId for retries so nothing is applied twice.
        - Destructive actions (deleting frames/layers, new project, resize) ask the human for approval.
        Coordinates: (0,0) is the top-left pixel. Colors: #RRGGBB or #RRGGBBAA.
    """.trimIndent()

    val PIXEL_ART_GUIDE = """
        # Pixel art guide for agents
        - Readability first: strong silhouette, 1px dark outline (not pure black — a darkened hue of the fill).
        - Limited palette: 3-5 shades per material; shift hue toward warm in highlights and cool in shadows.
        - Avoid "jaggies": lines should step in consistent runs (1-1-1, 2-2-2, 1-2-1...), not random lengths.
        - Avoid pillow shading (light from all edges); pick one light direction (usually top-left).
        - Anti-aliasing only on long curves, never on the outer silhouette for game sprites.
        - Animation: keep volume constant between frames; use squash/stretch sparingly at 16-64px.
          Typical timings: idle 4-6 frames @ 150-200ms, walk 6-8 frames @ 80-120ms, attack: short anticipation, fast strike, longer recovery.
        - Verify consistency by comparing frames with onion skin (canvas_snapshot with onion {"before":1}).
    """.trimIndent()

    class PromptDef(val name: String, val title: String, val description: String, val arguments: List<Triple<String, String, Boolean>>, val render: (Map<String, String>) -> String)

    val prompts = listOf(
        PromptDef(
            "draw_sprite", "Draw a sprite", "Step-by-step plan for drawing a character or object pixel by pixel.",
            listOf(Triple("subject", "What to draw, e.g. 'a samurai with a red scarf'", true), Triple("size", "Canvas size, e.g. 64x64", false)),
        ) { a ->
            "Using the Pixel Studio tools, draw ${a["subject"]} on a ${a["size"] ?: "the current"} canvas. " +
                "Start with canvas_inspect, plan a palette of at most 16 colors and the layers you need, block in the silhouette, " +
                "then refine shading and outline with set_pixels. Check the result with canvas_snapshot after each stage and fix problems you see."
        },
        PromptDef(
            "animate_sprite", "Animate a sprite", "Builds a frame-by-frame animation from the existing drawing.",
            listOf(Triple("animation", "Animation name, e.g. 'run'", true), Triple("frames", "Number of frames", false)),
        ) { a ->
            "Create a '${a["animation"]}' animation with ${a["frames"] ?: "an appropriate number of"} frames from the character on frame 0. " +
                "Plan the key poses first, duplicate the previous frame for each new pose, redraw only the moving parts, keep the character's " +
                "proportions and palette identical, set per-frame durations, tag the range with animation_tag_create and review it with animation_preview."
        },
        PromptDef("review_canvas", "Critique the canvas", "Reviews the current artwork and suggests concrete pixel-level fixes.", emptyList()) {
            "Look at the canvas with canvas_snapshot and read_region. Critique silhouette, palette, shading direction, outline quality and (if animated) " +
                "frame-to-frame consistency. List concrete fixes with coordinates, then apply them only after summarizing your plan."
        },
    )

    fun listPrompts(): List<Map<String, Any?>> = prompts.map { p ->
        linkedMapOf(
            "name" to p.name, "title" to p.title, "description" to p.description,
            "arguments" to p.arguments.map { (n, d, r) -> linkedMapOf("name" to n, "description" to d, "required" to r) },
        )
    }

    fun getPrompt(name: String, args: Map<String, Any?>): Map<String, Any?> {
        val p = prompts.firstOrNull { it.name == name } ?: throw RpcException(JsonRpc.INVALID_PARAMS, "unknown prompt '$name'")
        val strArgs = args.mapValues { it.value?.toString() ?: "" }
        p.arguments.filter { it.third }.forEach { (n, _, _) ->
            if (strArgs[n].isNullOrBlank()) throw RpcException(JsonRpc.INVALID_PARAMS, "missing prompt argument '$n'")
        }
        return linkedMapOf(
            "description" to p.description,
            "messages" to listOf(linkedMapOf("role" to "user", "content" to linkedMapOf("type" to "text", "text" to p.render(strArgs)))),
        )
    }
}
