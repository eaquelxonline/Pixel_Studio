package com.pixel.studio.gateway.tools

// Character-art finishing tools (fully typed, separate from the main catalogue builder).

private typealias Schema = Map<String, Any?>

internal fun MutableList<ToolDef>.addCharTools() {
    fun tool(name: String, title: String, access: Access, description: String, properties: Map<String, Schema>, required: List<String> = emptyList()) {
        add(ToolDef(name, title, description, name, properties, required, access, Output.JSON, "pixelart"))
    }
    val num: (String) -> Schema = { d -> linkedMapOf<String, Any?>("type" to "number", "description" to d) }
    val colors: (String) -> Schema = { d -> linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to d) }
    val light: Schema = linkedMapOf<String, Any?>(
        "type" to listOf("string", "array"),
        "description" to "Light direction: top-left (default), top-right, top, left, right, bottom, front, or [x, y] (image space, y down).",
    )
    val scope: Schema = string("cel (default) | layer (all frames) | frame (all layers) | all.", listOf("cel", "layer", "frame", "all"))

    tool("auto_shade", "Auto cel-shade", Access.WRITE,
        "Shades flat-colored shapes like an anime pixel artist: every same-color region gets hue-shifted 2-4 tone cel shading that follows its outline " +
            "(lit edges toward the light, shadow on the far side), layers ABOVE cast shadows onto it (bangs on the forehead, head on the neck), optional rim light. " +
            "Workflow: block the character in flat colors on separate layers (hair back, body, face, hair front), auto_shade each, outline mode=selout, then hand-paint eyes and highlights. " +
            "Regions smaller than min_region (eyes, buttons) and 'preserve' colors are untouched.",
        linkedMapOf<String, Schema>(
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer (default active)."),
            "frame" to integer("Frame index (default active).", min = 0), "scope" to scope, "light" to light,
            "tones" to integer("2 = base+shadow, 3 = +highlight (default), 4 = +deep shadow.", 2, 4),
            "bevel" to num("How far edge shading reaches into shapes, px (default 3)."),
            "form" to num("Whole-shape light→shadow gradient 0-2 (default 0.45)."),
            "shadow" to num("-1..1: more (positive) or less shadow."),
            "cast_shadow" to integer("Shadow length cast by layers above, px (default 2, 0 = off).", 0, 64),
            "hue_shift" to num("Auto ramp hue shift in degrees (default 22)."), "contrast" to num("Auto ramp contrast 0.05-1 (default 0.5; pastel ≈ 0.35)."),
            "ramps" to linkedMapOf<String, Any?>("type" to "object", "description" to "Explicit ramps: {\"#base\": [shadow, highlight] | [shadow, base, highlight] | [deep, shadow, base, highlight]}."),
            "preserve" to colors("Colors never shaded."), "min_region" to integer("Smallest region shaded, px (default 6).", 1),
            "rim" to color("Rim-light color on edges facing away from the light."), "rim_strength" to num("Rim blend 0-1 (default 0.55)."),
        ))
    tool("gradient_map", "Gradient map (color grade)", Access.WRITE,
        "Color-grades by mapping luminance onto a dark→light gradient (rainy blue night, sunset, sepia flashback, unified palettes), blended by strength.",
        linkedMapOf<String, Schema>("colors" to colors("2-32 colors from dark to light."), "strength" to num("0-1 (default 1)."), "scope" to scope,
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)),
        listOf("colors"))
    tool("draw_sparkle", "Sparkles / stars", Access.WRITE,
        "Stamps 4-point sparkles (star4), crosses, diamonds or dots — the cute twinkles around chibi characters, magic, item shine, starry skies.",
        linkedMapOf<String, Schema>("x" to integer("Center x."), "y" to integer("Center y."), "size" to integer("Arm length 1-8 (default 3).", 1, 8),
            "points" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "Many sparkles: [[x, y] or [x, y, size], …]."),
            "style" to string("Shape.", listOf("star4", "cross", "diamond", "dot")), "color" to color("Arm color (default #FFE9B0)."), "core" to color("Center color (default white)."),
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)))
    tool("pixel_cleanup", "Pixel-perfect lines", Access.WRITE,
        "Removes L-shaped corner doubles from 1-px lines (the 'pixel perfect' pass) so outlines and strands read clean.",
        linkedMapOf<String, Schema>("color" to color("Only lines of this color."), "passes" to integer("1-4.", 1, 4), "scope" to scope,
            "layer" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Layer."), "frame" to integer("Frame.", min = 0)))
}
