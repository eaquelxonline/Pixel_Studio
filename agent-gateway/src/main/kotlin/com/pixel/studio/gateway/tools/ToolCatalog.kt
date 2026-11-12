package com.pixel.studio.gateway.tools

/** How dangerous a tool is; drives the default permission decision. */
enum class Access { READ, WRITE, DESTRUCTIVE }

/** What the tool returns besides JSON. */
enum class Output { JSON, PNG_IMAGE, GIF_IMAGE, PROJECT_SAVE, PROJECT_EXPORT }

class ToolDef(
    val name: String,
    val title: String,
    val description: String,
    /** Engine operation executed by this tool. */
    val op: String,
    val properties: Map<String, Map<String, Any?>>,
    val required: List<String>,
    val access: Access,
    val output: Output = Output.JSON,
    val category: String,
) {
    /** JSON Schema (2020-12 compatible) for the arguments. */
    fun inputSchema(): Map<String, Any?> {
        val props = LinkedHashMap(properties)
        if (access != Access.READ) {
            props["expectedRevision"] = integer("Optimistic concurrency: fail with 'conflict' if the canvas revision differs (get it from canvas_inspect or any previous result).", min = 0)
            props["operationId"] = string("Idempotency key: retrying with the same id never applies the change twice.")
        }
        val schema = linkedMapOf<String, Any?>("type" to "object", "properties" to props)
        if (required.isNotEmpty()) schema["required"] = required
        schema["additionalProperties"] = false
        return schema
    }

    fun annotations(): Map<String, Any?> = linkedMapOf(
        "title" to title,
        "readOnlyHint" to (access == Access.READ),
        "destructiveHint" to (access == Access.DESTRUCTIVE),
        "idempotentHint" to (access == Access.READ),
        "openWorldHint" to false,
    )
}

// ---- schema helpers ---------------------------------------------------------------------

fun integer(description: String, min: Long? = null, max: Long? = null): Map<String, Any?> =
    linkedMapOf<String, Any?>("type" to "integer", "description" to description).apply {
        if (min != null) put("minimum", min)
        if (max != null) put("maximum", max)
    }

fun string(description: String, enum: List<String>? = null): Map<String, Any?> =
    linkedMapOf<String, Any?>("type" to "string", "description" to description).apply { if (enum != null) put("enum", enum) }

fun boolean(description: String): Map<String, Any?> = linkedMapOf("type" to "boolean", "description" to description)

fun color(description: String = "True color as #RRGGBB or #RRGGBBAA (any of 16.7M colors + 8-bit alpha), or 'transparent'."): Map<String, Any?> =
    linkedMapOf("type" to "string", "description" to description, "pattern" to "^(#?([0-9A-Fa-f]{3,4}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})|transparent)$")

private val LAYER = linkedMapOf<String, Any?>(
    "type" to listOf("integer", "string"),
    "description" to "Layer index (0 = bottom) or exact layer name. Defaults to the active layer.",
)
private val FRAME = integer("0-based frame index. Defaults to the active frame.", min = 0)
private val X = integer("X coordinate (0 = left).")
private val Y = integer("Y coordinate (0 = top).")

private fun target() = linkedMapOf("layer" to LAYER, "frame" to FRAME)

private fun region(): LinkedHashMap<String, Map<String, Any?>> = linkedMapOf(
    "x" to integer("Region left (default 0)."),
    "y" to integer("Region top (default 0)."),
    "width" to integer("Region width (default: to the canvas edge).", min = 1),
    "height" to integer("Region height (default: to the canvas edge).", min = 1),
)

private fun props(vararg groups: Map<String, Map<String, Any?>>): LinkedHashMap<String, Map<String, Any?>> =
    LinkedHashMap<String, Map<String, Any?>>().apply { groups.forEach { putAll(it) } }

/**
 * The tool catalogue shared by MCP (`tools/list`), REST (`/api/v1/tools`) and the
 * function-calling adapters. It is exported to `protocol/tools/pixel-studio-tools.json`.
 */
object ToolCatalog {
    val tools: List<ToolDef> = buildList {
        fun tool(
            name: String, title: String, category: String, access: Access, description: String,
            properties: Map<String, Map<String, Any?>> = emptyMap(), required: List<String> = emptyList(),
            op: String = name, output: Output = Output.JSON,
        ) = add(ToolDef(name, title, description, op, properties, required, access, output, category))

        // -- Canvas ---------------------------------------------------------------------
        tool("canvas_inspect", "Inspect canvas", "canvas", Access.READ,
            "Canvas size, current revision, layers, frames (with durations), tags and active layer/frame. Call this first and whenever you need the latest revision.")
        tool("canvas_snapshot", "Look at the canvas", "canvas", Access.READ,
            "Your eyes: returns a PNG image of a frame (all visible layers, upscaled for viewing) plus a JSON summary of content bounds and used colors. Use after drawing to check your work.",
            props(linkedMapOf("frame" to FRAME, "scale" to integer("Integer upscale factor 1-64 (default: auto to ~512px).", 1, 64),
                "onion" to linkedMapOf("type" to "object", "description" to "Onion skin: {before, after, opacity 0-255, tint}. Shows neighbouring frames faintly underneath.",
                    "properties" to linkedMapOf("before" to integer("Previous frames to show.", 0, 8), "after" to integer("Next frames to show.", 0, 8),
                        "opacity" to integer("Opacity of the nearest onion frame.", 0, 255), "tint" to boolean("Tint previous red and next blue."))),
                "background" to color("Optional background color for the image."))), output = Output.PNG_IMAGE)
        tool("get_pixel", "Read pixel", "canvas", Access.READ, "Reads one pixel's color from a layer (or the composited frame with composite=true).",
            props(linkedMapOf("x" to X, "y" to Y), target(), linkedMapOf("composite" to boolean("Read the final image of all visible layers instead of one layer."))), listOf("x", "y"))
        tool("read_region", "Read pixels", "canvas", Access.READ,
            "Reads a rectangle of pixels (max 128x128) as text. format 'ascii' (default) gives one string per row with a color legend ('.' = transparent) — ideal for reasoning about shapes; 'hex' gives exact colors.",
            props(region(), target(), linkedMapOf("format" to string("Output format.", listOf("ascii", "hex")), "composite" to boolean("Read the composited frame."))))
        tool("set_pixel", "Paint pixel", "canvas", Access.WRITE, "Paints a single pixel.",
            props(linkedMapOf("x" to X, "y" to Y, "color" to color()), target()), listOf("x", "y", "color"))
        tool("set_pixels", "Paint pixels", "canvas", Access.WRITE,
            "Paints many explicit pixels in one undoable step. Each entry is [x, y, color] or {x, y, color}; entries without a color use 'color'. Use null/'transparent' to erase. Up to 262144 pixels.",
            props(linkedMapOf("pixels" to linkedMapOf("type" to "array", "description" to "Pixels: [[x, y, \"#RRGGBB\"], ...] or [{\"x\":..,\"y\":..,\"color\":..}, ...].",
                "items" to linkedMapOf("type" to listOf("array", "object"))), "color" to color("Default color for entries without one.")), target()), listOf("pixels"))
        tool("erase_pixel", "Erase pixel", "canvas", Access.WRITE, "Makes one pixel transparent.",
            props(linkedMapOf("x" to X, "y" to Y), target()), listOf("x", "y"))
        tool("draw_line", "Draw line", "canvas", Access.WRITE, "Draws a 1-pixel Bresenham line including both end points.",
            props(linkedMapOf("x0" to X, "y0" to Y, "x1" to X, "y1" to Y, "color" to color()), target()), listOf("x0", "y0", "x1", "y1", "color"))
        tool("draw_rectangle", "Draw rectangle", "canvas", Access.WRITE, "Draws a rectangle outline, or a filled rectangle with filled=true.",
            props(linkedMapOf("x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1), "color" to color(), "filled" to boolean("Fill the inside.")), target()),
            listOf("x", "y", "width", "height", "color"))
        tool("draw_ellipse", "Draw ellipse", "canvas", Access.WRITE, "Draws a pixel-perfect ellipse inscribed in the given box (outline or filled).",
            props(linkedMapOf("x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1), "color" to color(), "filled" to boolean("Fill the inside.")), target()),
            listOf("x", "y", "width", "height", "color"))
        tool("fill_region", "Fill area", "canvas", Access.WRITE, "Bucket fill: replaces the connected area of the seed pixel's color (4-connected). contiguous=false replaces that color everywhere on the layer.",
            props(linkedMapOf("x" to X, "y" to Y, "color" to color(), "contiguous" to boolean("Default true.")), target()), listOf("x", "y", "color"))
        tool("replace_color", "Replace color", "canvas", Access.WRITE, "Replaces every pixel of one color with another in a cel, a layer (all frames), a frame (all layers) or everywhere.",
            props(linkedMapOf("from" to color("Color to replace."), "to" to color("New color."), "scope" to string("Where to replace (default 'cel').", listOf("cel", "layer", "frame", "all"))), target()),
            listOf("from", "to"))
        tool("flip_horizontal", "Flip horizontally", "canvas", Access.WRITE, "Mirrors a region (default whole layer) left↔right.", props(region(), target()))
        tool("flip_vertical", "Flip vertically", "canvas", Access.WRITE, "Mirrors a region (default whole layer) top↕bottom.", props(region(), target()))
        tool("rotate", "Rotate", "canvas", Access.WRITE, "Rotates a region clockwise by 90/180/270 degrees (90/270 need a square region).",
            props(linkedMapOf("degrees" to linkedMapOf("type" to "integer", "enum" to listOf(90, 180, 270), "description" to "Clockwise rotation.")), region(), target()), listOf("degrees"))
        tool("move_region", "Move pixels", "canvas", Access.WRITE, "Moves the pixels of a region by (dx, dy) on the same layer. mode 'over' keeps destination pixels under transparent source pixels.",
            props(region(), linkedMapOf("dx" to integer("Horizontal offset."), "dy" to integer("Vertical offset."), "mode" to string("Paste mode (default 'over').", listOf("over", "replace"))), target()),
            listOf("dx", "dy"))
        tool("copy_region", "Copy pixels", "canvas", Access.WRITE,
            "Copies a region to another position, layer and/or frame. Ideal for building a new animation frame from the previous pose, then editing only what moves.",
            props(region(), target(), linkedMapOf("toX" to integer("Destination left (default: same x)."), "toY" to integer("Destination top (default: same y)."),
                "toLayer" to LAYER, "toFrame" to integer("Destination frame index.", 0), "mode" to string("Paste mode (default 'over').", listOf("over", "replace")))))
        tool("clear", "Clear", "canvas", Access.WRITE, "Erases a region (default: the whole cel).", props(region(), target()))
        tool("canvas_resize", "Resize canvas", "canvas", Access.DESTRUCTIVE, "Changes the canvas size; pixels outside the new bounds are cropped. offsetX/offsetY shift existing art.",
            linkedMapOf("width" to integer("New width.", 1, 4096), "height" to integer("New height.", 1, 4096), "offsetX" to integer("Shift existing pixels right."), "offsetY" to integer("Shift existing pixels down.")),
            listOf("width", "height"))

        // -- Animation ------------------------------------------------------------------
        tool("frame_add", "Add frames", "animation", Access.WRITE, "Adds one or more empty frames (no fixed limit). Defaults: appended at the end, duration of the active frame.",
            linkedMapOf("index" to integer("Insert position.", 0), "count" to integer("How many frames (1-10000).", 1, 10000), "duration" to integer("Duration in ms (1-60000).", 1, 60000)))
        tool("frame_duplicate", "Duplicate frame", "animation", Access.WRITE, "Copies a frame with all its layers (stored efficiently until edited). Default target: right after the source.",
            linkedMapOf("frame" to FRAME, "index" to integer("Position of the copy.", 0)))
        tool("frame_delete", "Delete frame", "animation", Access.DESTRUCTIVE, "Deletes a frame and its pixels. Requires the user's approval.",
            linkedMapOf("frame" to integer("Frame index to delete.", 0)), listOf("frame"))
        tool("frame_move", "Move frame", "animation", Access.WRITE, "Reorders a frame.", linkedMapOf("frame" to FRAME, "index" to integer("New position.", 0)), listOf("index"))
        tool("frame_get", "Frame details", "animation", Access.READ, "Duration, tags and per-layer pixel counts/bounds of a frame.", linkedMapOf("frame" to FRAME))
        tool("frame_set_duration", "Set frame duration", "animation", Access.WRITE, "Sets how long a frame is shown (ms). frame='all' sets every frame.",
            linkedMapOf("frame" to linkedMapOf("type" to listOf("integer", "string"), "description" to "Frame index, or \"all\"."), "duration" to integer("Milliseconds (1-60000).", 1, 60000)),
            listOf("duration"))
        tool("frame_select", "Select frame", "animation", Access.WRITE, "Makes a frame the active one (shown in the editor).", linkedMapOf("frame" to FRAME), listOf("frame"))
        tool("animation_tag_create", "Create animation tag", "animation", Access.WRITE, "Names a frame range as an animation (e.g. Idle, Walk, Attack) with a playback direction.",
            linkedMapOf("name" to string("Tag name."), "from" to integer("First frame index.", 0), "to" to integer("Last frame index.", 0),
                "direction" to string("Playback direction.", listOf("forward", "reverse", "pingpong")), "repeat" to integer("0 = loop forever.", 0, 65535)),
            listOf("name", "from", "to"))
        tool("animation_tag_delete", "Delete animation tag", "animation", Access.WRITE, "Removes a tag (frames are kept).", linkedMapOf("name" to string("Tag name.")), listOf("name"))
        tool("animation_set_loop", "Set loop mode", "animation", Access.WRITE, "Sets a tag's direction (forward/reverse/pingpong) and repeat count.",
            linkedMapOf("tag" to string("Tag name."), "direction" to string("Direction.", listOf("forward", "reverse", "pingpong")), "repeat" to integer("0 = loop forever.", 0, 65535)),
            listOf("tag"))
        tool("animation_info", "Animation timing", "animation", Access.READ, "Playback order, durations, total length and average FPS for a tag or frame range.",
            linkedMapOf("tag" to string("Tag name."), "from" to integer("First frame.", 0), "to" to integer("Last frame.", 0), "direction" to string("Direction.", listOf("forward", "reverse", "pingpong"))))
        tool("animation_preview", "Preview animation", "animation", Access.READ, "Renders the animation (or a tag) as an animated GIF so you can review motion.",
            linkedMapOf("tag" to string("Tag name."), "from" to integer("First frame.", 0), "to" to integer("Last frame.", 0), "scale" to integer("Upscale 1-64.", 1, 64)),
            op = "export_gif", output = Output.GIF_IMAGE)

        // -- Layers ---------------------------------------------------------------------
        tool("layer_create", "Create layer", "layers", Access.WRITE, "Adds a layer (default: above the active one) and selects it. Split characters into parts (Body, Head, Weapon, ...) so they can be edited independently.",
            linkedMapOf("name" to string("Layer name."), "index" to integer("Position (0 = bottom).", 0)))
        tool("layer_delete", "Delete layer", "layers", Access.DESTRUCTIVE, "Deletes a layer and all its pixels in every frame. Requires the user's approval.",
            linkedMapOf("layer" to LAYER), listOf("layer"))
        tool("layer_rename", "Rename layer", "layers", Access.WRITE, "Renames a layer.", linkedMapOf("layer" to LAYER, "name" to string("New name.")), listOf("name"))
        tool("layer_visibility", "Show/hide layer", "layers", Access.WRITE, "Shows or hides a layer.", linkedMapOf("layer" to LAYER, "visible" to boolean("Visible?")), listOf("visible"))
        tool("layer_lock", "Lock layer", "layers", Access.WRITE, "Locks/unlocks a layer against edits.", linkedMapOf("layer" to LAYER, "locked" to boolean("Locked?")), listOf("locked"))
        tool("layer_opacity", "Layer opacity", "layers", Access.WRITE, "Sets layer opacity 0-255.", linkedMapOf("layer" to LAYER, "opacity" to integer("0-255.", 0, 255)), listOf("opacity"))
        tool("layer_move", "Move layer", "layers", Access.WRITE, "Changes a layer's stacking position.", linkedMapOf("layer" to LAYER, "index" to integer("New position (0 = bottom).", 0)), listOf("index"))
        tool("layer_select", "Select layer", "layers", Access.WRITE, "Makes a layer the active one.", linkedMapOf("layer" to LAYER), listOf("layer"))

        // -- Palette --------------------------------------------------------------------
        tool("palette_get", "Get swatches", "palette", Access.READ, "Lists the project's color swatches with their indices. The canvas itself is true color and not limited to them.")
        tool("palette_set", "Change swatch", "palette", Access.WRITE, "Changes a swatch; every pixel that used the old color is recolored (great for color variants).",
            linkedMapOf("index" to integer("Palette index (1+).", 1), "color" to color()), listOf("index", "color"))
        tool("palette_add", "Add swatches", "palette", Access.WRITE, "Adds colors to the swatch list (plan a palette before drawing).",
            linkedMapOf("colors" to linkedMapOf("type" to "array", "items" to color(), "description" to "Hex colors.")), listOf("colors"))

        // -- Project --------------------------------------------------------------------
        tool("project_inspect", "Inspect project", "project", Access.READ, "Like canvas_inspect, plus per-layer pixel counts and storage statistics.")
        tool("project_save", "Save project", "project", Access.WRITE, "Saves the project (.pxs) to its file in the Pixel Studio workspace.",
            linkedMapOf("name" to string("Optional file name (letters, digits, - and _ only).")), op = "project_save", output = Output.PROJECT_SAVE)
        tool("project_export", "Export", "project", Access.WRITE,
            "Exports PNG (one frame), GIF (animation) or sprite sheet (PNG + JSON metadata for Godot/Unity) into the workspace's Exports folder and returns the image.",
            linkedMapOf("format" to string("Export format.", listOf("png", "gif", "spritesheet")), "name" to string("File name without extension."),
                "frame" to FRAME, "tag" to string("Tag to export (gif/spritesheet)."), "scale" to integer("Upscale 1-64.", 1, 64),
                "columns" to integer("Sprite sheet columns (0 = auto).", 0), "padding" to integer("Sprite sheet padding in px.", 0, 64)),
            listOf("format"), op = "export", output = Output.PROJECT_EXPORT)
        tool("project_new", "New project", "project", Access.DESTRUCTIVE, "Replaces the current project with a new blank canvas (undoable). Requires the user's approval.",
            linkedMapOf("width" to integer("Width 1-4096.", 1, 4096), "height" to integer("Height 1-4096.", 1, 4096), "name" to string("Project name.")), listOf("width", "height"))
        tool("project_rename", "Rename project", "project", Access.WRITE, "Renames the project.", linkedMapOf("name" to string("New name.")), listOf("name"))
        tool("history_undo", "Undo", "project", Access.WRITE, "Undoes the last change(s), whoever made them.", linkedMapOf("steps" to integer("1-256.", 1, 256)))
        tool("history_redo", "Redo", "project", Access.WRITE, "Redoes undone change(s).", linkedMapOf("steps" to integer("1-256.", 1, 256)))
        tool("history_list", "History", "project", Access.READ, "Recent operations with actor, revision and changed pixel counts.", linkedMapOf("limit" to integer("1-1000.", 1, 1000)))
        tool("transaction", "Atomic batch", "project", Access.WRITE,
            "Runs several drawing/structure commands as ONE atomic, undoable step: if any fails, nothing is applied. Each command is {\"op\": <tool name>, ...its arguments}.",
            linkedMapOf("commands" to linkedMapOf("type" to "array", "description" to "Commands, e.g. [{\"op\":\"draw_line\",\"x0\":0,...}, {\"op\":\"set_pixel\",...}].",
                "items" to linkedMapOf("type" to "object", "required" to listOf("op"), "properties" to linkedMapOf("op" to string("Operation name."))))),
            listOf("commands"))
        tool("image_import", "Import image", "project", Access.WRITE, "Pastes a PNG (base64) into a layer/frame at x,y — reference art, imported sprites.",
            props(linkedMapOf("data_base64" to string("PNG file as base64."), "x" to X, "y" to Y), target()), listOf("data_base64"))

        // -- 3D pixel art (voxels) ---------------------------------------------------------
        val model = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Model index or name (default: the first model).")
        val vec3 = { d: String -> linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf("type" to "number"), "minItems" to 3, "maxItems" to 3, "description" to d) }
        val boneRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Bone name or index the voxels follow when animated.")
        val materialRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "PBR material name or index (default material 0).")
        val look = linkedMapOf(
            "yaw" to linkedMapOf<String, Any?>("type" to "number", "description" to "Turntable angle in degrees; 0 = front faces the camera, 90 = character faces screen-right."),
            "pitch" to linkedMapOf<String, Any?>("type" to "number", "description" to "Camera elevation in degrees (default 30; 0 = side view)."),
            "width" to integer("Output width in pixels (8-2048, default 256).", 8, 2048),
            "height" to integer("Output height (default = width).", 8, 2048),
            "scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Pixels per voxel. Omit to auto-fit the model."),
            "focus" to vec3("Model point to center in the image (close-ups, with scale)."),
            "animation" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "3D animation name/index."),
            "frame" to linkedMapOf<String, Any?>("type" to "number", "description" to "Animation time in frames (fractions allowed)."),
            "pass" to string("Render pass / G-buffer map.", listOf("lit", "albedo", "normal", "depth", "occlusion", "roughness", "metallic", "emission")),
            "smoothness" to linkedMapOf<String, Any?>("type" to "number", "description" to "0 = crisp voxel faces (3D pixel look), 1 = smooth sprite shading (default 0.55)."),
            "toon_steps" to integer("0 = physically based; 2-16 = toon light bands.", 0, 16),
            "outline" to linkedMapOf<String, Any?>("type" to listOf("boolean", "string"), "description" to "true/'selective' = pixel-art selective outline, or a hex outline color."),
            "inner_lines" to boolean("Darken depth edges inside the silhouette (separates limbs)."),
            "supersample" to integer("1-4. 2 gives clean edges at small sprite sizes.", 1, 4),
            "exposure" to linkedMapOf<String, Any?>("type" to "number", "description" to "Lighting exposure (default 1)."),
            "background" to color("Optional background color."),
            "background_gradient" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "minItems" to 2, "maxItems" to 2, "description" to "[top, bottom] colors with a soft vignette (diorama look)."),
            "shadows" to linkedMapOf<String, Any?>("type" to "number", "description" to "Cast shadows from the key light, 0-1."),
            "ground_shadow" to linkedMapOf<String, Any?>("type" to "number", "description" to "Shadow on the ground under the model for transparent sprites (alpha 0-1)."),
            "bloom" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow around emissive voxels (0-4; 0.5-0.8 typical)."),
            "bloom_radius" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow radius in output pixels (0 = auto)."),
            "lights" to linkedMapOf<String, Any?>("type" to "array", "description" to "Point lights: [{position:[x,y,z], color, intensity, radius}] in model space (fires, windows, magic).",
                "items" to linkedMapOf("type" to "object")),
            "key_light" to vec3("Key light direction (view space, toward the light)."),
            "key_intensity" to linkedMapOf<String, Any?>("type" to "number", "description" to "Key light intensity (default 3.2)."),
            "ambient" to linkedMapOf<String, Any?>("type" to "number", "description" to "Ambient multiplier (default 1; lower for night scenes)."),
        )
        val sequence = linkedMapOf<String, Map<String, Any?>>(
            "models" to linkedMapOf("type" to "array", "items" to linkedMapOf("type" to "string"), "description" to "Frame-by-frame 3D animation: each model is one frame (VFX, morphs). One shared camera keeps it stable."),
            "name" to string("Sequence name (tags become <name>_<direction>)."),
            "frame_ms" to integer("Milliseconds per frame for sequences.", 1, 60000),
        )
        tool("voxel_model_create", "Create 3D model", "3d", Access.WRITE,
            "Creates a sparse voxel model (3D pixels), up to 512x512x512. y is up, the model's front faces +z, origin = point between the feet (default bottom center).",
            linkedMapOf("name" to string("Model name."), "width" to integer("X size.", 1, 512), "height" to integer("Y size (up).", 1, 512), "depth" to integer("Z size.", 1, 512), "origin" to vec3("Ground pivot [x, y, z].")),
            listOf("name"))
        tool("voxel_model_clone", "Clone 3D model", "3d", Access.WRITE,
            "Duplicates a model (voxels, rig, animations). Build a static base once, clone it per frame and add the moving voxels: frame-by-frame VFX (fire, smoke, magic). empty=true clones only the rig/size.",
            linkedMapOf("model" to model, "name" to string("New model name."), "empty" to boolean("Copy without voxels.")), listOf("name"))
        tool("voxel_model_delete", "Delete 3D model", "3d", Access.DESTRUCTIVE, "Deletes a voxel model. Requires the user's approval.", linkedMapOf("model" to model), listOf("model"))
        tool("voxel_model_set", "Rename / re-pivot model", "3d", Access.WRITE, "Renames a model or moves its origin pivot.", linkedMapOf("model" to model, "name" to string("New name."), "origin" to vec3("New origin.")))
        tool("voxel_inspect", "Inspect 3D project", "3d", Access.READ, "Models (size, voxel count, bounds, skeleton, animations), PBR materials and textures.", linkedMapOf("model" to model))
        tool("voxel_get", "Read voxel", "3d", Access.READ, "Color, material and bone of one voxel.", linkedMapOf("model" to model, "x" to X, "y" to Y, "z" to integer("Z coordinate.")), listOf("x", "y", "z"))
        tool("voxel_set", "Place voxel", "3d", Access.WRITE, "Places (or with 'transparent' removes) one voxel.",
            linkedMapOf("model" to model, "x" to X, "y" to Y, "z" to integer("Z coordinate."), "color" to color(), "material" to materialRef, "bone" to boneRef), listOf("x", "y", "z", "color"))
        tool("voxel_set_many", "Place voxels", "3d", Access.WRITE, "Places up to 1,000,000 explicit voxels in one undoable step: [[x, y, z, color], ...] or objects with material/bone.",
            linkedMapOf("model" to model, "voxels" to linkedMapOf("type" to "array", "items" to linkedMapOf("type" to listOf("array", "object")), "description" to "Voxels."), "color" to color("Default color."), "material" to materialRef, "bone" to boneRef),
            listOf("voxels"))
        tool("voxel_shape", "Sculpt shape", "3d", Access.WRITE,
            "Sculpts or paints with a primitive. shape: box(min,max) | ellipsoid(center, radii or radius) | capsule(a, b, radius, radius_b — tapered limbs) | cone | cylinder(a, b, radius). mode: add | paint (recolor existing surface only) | erase. Extras: color_to + gradient_axis (gradients), jitter 0-1 (tonal noise for texture), mirror_x (symmetric, swaps _l/_r bones), material, bone.",
            linkedMapOf("model" to model, "shape" to string("Primitive.", listOf("box", "ellipsoid", "sphere", "capsule", "cone", "cylinder")), "mode" to string("Operation.", listOf("add", "paint", "erase")),
                "min" to vec3("Box min corner."), "max" to vec3("Box max corner (inclusive)."), "center" to vec3("Ellipsoid center."), "radii" to vec3("Ellipsoid radii."),
                "radius" to linkedMapOf<String, Any?>("type" to "number", "description" to "Radius (capsule start / sphere / cylinder)."), "radius_b" to linkedMapOf<String, Any?>("type" to "number", "description" to "Capsule end radius (taper)."),
                "a" to vec3("Capsule/cylinder start."), "b" to vec3("Capsule/cylinder end."), "color" to color(), "color_to" to color("Gradient end color."),
                "gradient_axis" to string("Gradient axis.", listOf("x", "y", "z")), "jitter" to linkedMapOf<String, Any?>("type" to "number", "description" to "Tonal noise 0-1."), "seed" to integer("Noise seed."),
                "mirror_x" to boolean("Also apply mirrored across the origin's X."), "material" to materialRef, "bone" to boneRef),
            listOf("shape"))
        tool("voxel_mirror", "Mirror model half", "3d", Access.WRITE, "Copies one half onto the other across the origin's X plane (bones _l/_r swapped).",
            linkedMapOf("model" to model, "from" to string("Source half.", listOf("+", "-"))))
        tool("material_set", "Create/edit PBR material", "3d", Access.WRITE,
            "Creates or updates a PBR material: roughness, metallic, emission, uv_scale (texels per voxel), normal_strength, maps {albedo, normal, roughness, metallic, occlusion, height, emission: texture name}.",
            linkedMapOf("name" to string("Material name."), "roughness" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-1."), "metallic" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-1."),
                "emission" to linkedMapOf<String, Any?>("type" to "number", "description" to "Glow strength (0 = none)."), "uv_scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Texels per voxel."),
                "normal_strength" to linkedMapOf<String, Any?>("type" to "number", "description" to "0-4."), "maps" to linkedMapOf<String, Any?>("type" to "object", "description" to "Slot → texture name.")),
            listOf("name"))
        tool("texture_import", "Import texture", "3d", Access.WRITE, "Imports a PNG (base64) as a texture map usable in any PBR slot.",
            linkedMapOf("name" to string("Texture name."), "data_base64" to string("PNG as base64.")), listOf("name", "data_base64"))
        tool("texture_generate", "Generate PBR textures", "3d", Access.WRITE,
            "Generates a tileable PBR set (albedo, normal, roughness, occlusion, height) and a material of the same name. kinds: noise, brushed_metal, hammered_metal, leather, fabric, scales, stone, wood, chainmail.",
            linkedMapOf("name" to string("Set/material name."), "kind" to string("Surface type.", listOf("noise", "brushed_metal", "hammered_metal", "leather", "fabric", "scales", "stone", "wood", "chainmail")),
                "size" to integer("Texture size (8-1024).", 8, 1024), "cells" to integer("Feature count across the tile.", 1, 64), "seed" to integer("Seed."),
                "roughness" to linkedMapOf<String, Any?>("type" to "number", "description" to "Base roughness."), "metallic" to linkedMapOf<String, Any?>("type" to "number", "description" to "Metallic."),
                "contrast" to linkedMapOf<String, Any?>("type" to "number", "description" to "Albedo variation 0-1."), "uv_scale" to linkedMapOf<String, Any?>("type" to "number", "description" to "Texels per voxel.")),
            listOf("name", "kind"))
        tool("rig_bone_add", "Add bone", "3d", Access.WRITE,
            "Adds a bone (pivot head → tail). Humanoid names enable the motion generators: hips, spine, chest, neck, head, upper_arm_l/r, lower_arm_l/r, hand_l/r, upper_leg_l/r, lower_leg_l/r, foot_l/r, cape. Character's left = +x.",
            linkedMapOf("model" to model, "name" to string("Bone name."), "parent" to boneRef, "head" to vec3("Pivot (joint) position."), "tail" to vec3("End position.")), listOf("name", "head"))
        tool("rig_clear", "Clear rig", "3d", Access.DESTRUCTIVE, "Removes all bones, animations and bindings of a model. Requires approval.", linkedMapOf("model" to model))
        tool("rig_bind", "Bind voxels to bones", "3d", Access.WRITE, "Binds voxels to their nearest bone (only_unbound keeps explicit bindings from voxel_shape's bone).",
            linkedMapOf("model" to model, "only_unbound" to boolean("Keep explicit bindings.")))
        tool("anim3d_create", "Create 3D animation", "3d", Access.WRITE, "Creates an empty keyframe animation.",
            linkedMapOf("model" to model, "name" to string("Animation name."), "frames" to integer("Length in frames.", 1, 10000), "frame_ms" to integer("Milliseconds per frame.", 1, 60000), "loop" to boolean("Loops.")), listOf("name"))
        tool("anim3d_key", "Set bone key", "3d", Access.WRITE, "Sets a bone's rotation (Euler degrees around its head) and position offset at a frame; frames between keys are interpolated.",
            linkedMapOf("model" to model, "animation" to linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Animation."), "bone" to boneRef,
                "frame" to linkedMapOf<String, Any?>("type" to "number", "description" to "Frame."), "rotation" to vec3("Euler degrees [x, y, z]."), "position" to vec3("Offset [x, y, z].")),
            listOf("animation", "bone", "frame"))
        tool("anim3d_generate", "Generate motion cycle", "3d", Access.WRITE, "Generates a humanoid walk, run or idle cycle (foot planting, counter-swing, cape lag) from standard bone names.",
            linkedMapOf("model" to model, "kind" to string("Cycle.", listOf("walk", "run", "idle")), "name" to string("Animation name (default = kind)."), "frames" to integer("Frames (2-240).", 2, 240),
                "frame_ms" to integer("ms per frame.", 1, 60000), "stride" to linkedMapOf<String, Any?>("type" to "number", "description" to "Leg swing degrees."), "knee" to linkedMapOf<String, Any?>("type" to "number", "description" to "Knee bend degrees."),
                "arm_swing" to linkedMapOf<String, Any?>("type" to "number", "description" to "Arm swing degrees."), "bob" to linkedMapOf<String, Any?>("type" to "number", "description" to "Extra vertical bob (voxels)."),
                "lean" to linkedMapOf<String, Any?>("type" to "number", "description" to "Forward lean degrees.")),
            listOf("kind"))
        tool("voxel_render", "Render 3D model", "3d", Access.READ, "Renders a model to PNG from any angle, animation frame and pass (lit PBR or a G-buffer map). Use it to look at your 3D work.",
            props(linkedMapOf("model" to model), look), op = "voxel_render", output = Output.PNG_IMAGE)
        tool("voxel_render_sheet", "Render sprite sheet", "3d", Access.READ,
            "Renders directions × animation frames into one sprite sheet (rows = directions S, SE, E, NE, N, NW, W, SW) for any pass — e.g. normal-map sheets for 2D dynamic lighting.",
            props(linkedMapOf("model" to model, "directions" to integer("1, 2, 4, 8 or 16.", 1, 16)), sequence, look), op = "voxel_render_sheet", output = Output.PNG_IMAGE)
        tool("voxel_render_to_frames", "Bake 3D to 2D frames", "3d", Access.WRITE,
            "Bakes directions × animation frames into the 2D project as editable pixel frames on one layer, tagged per direction (e.g. walk_S, walk_SE). All sprites share one scale and ground line.",
            props(linkedMapOf("model" to model, "directions" to integer("1, 2, 4, 8 or 16.", 1, 16), "layer" to string("Target layer name (default '3D Render')."), "resize_canvas" to boolean("Resize the 2D canvas to the render size (default true).")), sequence, look))
        tool("voxel_pick", "Pick voxel at pixel", "3d", Access.READ, "Which voxel and face is under image pixel (px, py) for the given camera — used by 3D viewports for click editing.",
            props(linkedMapOf("model" to model, "px" to integer("Pixel x."), "py" to integer("Pixel y.")), look), listOf("px", "py"))
    }

    private val byName = tools.associateBy { it.name }

    operator fun get(name: String): ToolDef? = byName[name]

    /** Access level of a raw engine operation (REST passthrough), by matching tool. */
    fun accessForOp(op: String): Access = tools.firstOrNull { it.op == op }?.access ?: when (op) {
        "project_load" -> Access.DESTRUCTIVE
        "status", "render_frame", "export_png", "export_gif", "export_spritesheet" -> Access.READ
        else -> Access.WRITE
    }
}
