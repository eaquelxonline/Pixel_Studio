package com.pixel.studio.gateway.tools

// Game-production tools, kept apart from the main catalogue (and fully typed) so the Kotlin
// compiler does not have to infer one enormous builder lambda.

internal fun MutableList<ToolDef>.addGameTools() {
    fun tool(
        name: String, title: String, category: String, access: Access, description: String,
        properties: Map<String, Map<String, Any?>> = emptyMap(), required: List<String> = emptyList(),
        op: String = name, output: Output = Output.JSON,
    ) {
        add(ToolDef(name, title, description, op, properties, required, access, output, category))
    }

    val num: (String) -> Map<String, Any?> = { d -> linkedMapOf<String, Any?>("type" to "number", "description" to d) }
    val tilesetRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tileset name or index (optional when there is only one).")
    val tilemapRef = linkedMapOf<String, Any?>("type" to listOf("integer", "string"), "description" to "Tilemap name or index (optional when there is only one).")
    val pack: LinkedHashMap<String, Map<String, Any?>> = linkedMapOf(
        "tag" to string("Only this tag's frames (default: all frames)."), "layer" to LAYER, "scale" to integer("Upscale 1-64.", 1, 64),
        "trim" to boolean("Trim transparent borders (default true)."), "padding" to integer("Gap between sprites (default 1).", 0, 64),
        "extrude" to integer("Edge pixels repeated around each sprite against bleeding (default 0).", 0, 16), "power_of_two" to boolean("Power-of-two atlas size."),
    )
    tool("layer_blend", "Layer blend mode", "gamedev", Access.WRITE,
        "Sets a layer's blend mode: normal, multiply (shadows, ambient occlusion), screen/add (glow, fire light, magic), overlay/soft_light (color grading), darken, lighten.",
        linkedMapOf<String, Map<String, Any?>>("layer" to LAYER, "mode" to string("Blend mode.", listOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light"))), listOf("mode"))
    tool("slice_set", "Set slice / hitbox", "gamedev", Access.WRITE,
        "Creates or updates a named per-frame rectangle (Aseprite-compatible slice): hitbox, hurtbox, pivot, weapon attach point. A key without 'frame' covers the whole animation; later keys override from their frame on. Exported in export_atlas meta.slices.",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Slice name, e.g. hitbox, hurtbox, pivot."), "frame" to FRAME, "x" to X, "y" to Y, "width" to integer("Width.", 1), "height" to integer("Height.", 1),
        "pivot" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "integer"), "minItems" to 2, "maxItems" to 2, "description" to "Pivot [x, y] relative to the slice."),
        "color" to color("Editor color of the slice.")),
        listOf("name", "x", "y", "width", "height"))
    tool("slice_delete", "Delete slice", "gamedev", Access.WRITE, "Deletes a slice, or only its key at 'frame'.", linkedMapOf<String, Map<String, Any?>>("name" to string("Slice name."), "frame" to FRAME), listOf("name"))
    tool("palette_swap", "Palette swap", "gamedev", Access.WRITE,
        "Remaps colors (RGB match, alpha kept) across a cel/layer/frame/everything: team colors, enemy variants, day/night palettes, damage flashes.",
        props(linkedMapOf<String, Map<String, Any?>>("map" to linkedMapOf<String, Any?>("type" to "object", "description" to "{\"#from\": \"#to\", …}"),
        "from" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Source colors (with 'to')."),
        "to" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Target colors, same length as 'from'."),
        "scope" to string("cel | layer | frame | all (default cel).", listOf("cel", "layer", "frame", "all"))), target()))
    tool("rotsprite", "RotSprite rotate", "gamedev", Access.WRITE,
        "Rotates a cel by any angle with the RotSprite algorithm (Scale2x ×8 + nearest): clean lines, no blur, no new colors — swords, projectiles, swing frames.",
        props(linkedMapOf<String, Map<String, Any?>>("degrees" to num("Clockwise degrees."), "center" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "number"), "minItems" to 2, "maxItems" to 2, "description" to "Pivot [x, y] (default: content center).")), target()),
        listOf("degrees"))
    tool("tileset_create", "Create tileset", "gamedev", Access.WRITE, "Creates an empty tileset (fill it with tileset_add_tiles).",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Tileset name."), "tile_size" to integer("Square tile size (default 16).", 1, 256), "tile_width" to integer("Tile width.", 1, 256), "tile_height" to integer("Tile height.", 1, 256)), listOf("name"))
    tool("tileset_add_tiles", "Cut tiles from canvas", "gamedev", Access.WRITE,
        "Cuts a grid of tiles from a rendered frame (or one layer) into a tileset; identical and empty tiles are skipped. Returns the tile index of each cut cell.",
        props(linkedMapOf<String, Map<String, Any?>>("tileset" to tilesetRef, "x" to X, "y" to Y, "columns" to integer("Tiles across.", 1, 256), "rows" to integer("Tiles down.", 1, 256),
        "dedupe" to boolean("Reuse identical tiles (default true)."), "skip_empty" to boolean("Skip empty cells (default true).")), target()))
    tool("tileset_generate_blob", "Generate 47-tile autotile set", "gamedev", Access.WRITE,
        "Synthesizes a complete 47-tile blob autotile terrain set (Godot terrains / Tiled Wang / LDtk / GameMaker layout) from one seamless fill: a canvas region (source_x/source_y) or a procedural fill color with accent clusters. Lit top edges, shaded sides, selective outline, rounded corners, optional cliff lip.",
        props(linkedMapOf<String, Map<String, Any?>>("name" to string("Tileset name."), "tile_size" to integer("Tile size (default 16).", 4, 256),
        "source_x" to integer("Take the fill tile from the canvas at this x."), "source_y" to integer("…and y."),
        "fill" to color("Procedural base color."), "accents" to linkedMapOf<String, Any?>("type" to "array", "items" to color(), "description" to "Accent colors (default: lighter/darker ramp steps)."),
        "density" to num("Accent density 0-1 (default 0.12)."), "seed" to integer("Pattern seed."),
        "light" to color("Lit top edge."), "dark" to color("Shaded edge."), "outline" to color("Outline (default: selective darker hue)."),
        "inset" to integer("Transparent margin on open sides (default 1).", 0, 64), "edge" to integer("Edge band width (default 1).", 0, 64),
        "lip" to integer("Cliff lip height under open south edges (default 0).", 0, 128), "lip_color" to color("Cliff lip color."), "rounded" to boolean("Round outer corners (default true).")),
        linkedMapOf<String, Map<String, Any?>>("frame" to FRAME, "layer" to LAYER)),
        listOf("name"))
    tool("tileset_delete", "Delete tileset", "gamedev", Access.DESTRUCTIVE, "Deletes an unused tileset.", linkedMapOf<String, Map<String, Any?>>("tileset" to tilesetRef))
    tool("tilemap_create", "Create tilemap", "gamedev", Access.WRITE, "Creates an empty tilemap (grid of tile cells) bound to a tileset.",
        linkedMapOf<String, Map<String, Any?>>("name" to string("Tilemap name."), "tileset" to tilesetRef, "columns" to integer("Width in tiles.", 1, 4096), "rows" to integer("Height in tiles.", 1, 4096)), listOf("name", "columns", "rows"))
    tool("tilemap_delete", "Delete tilemap", "gamedev", Access.DESTRUCTIVE, "Deletes a tilemap.", linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef))
    tool("tilemap_set", "Place tiles", "gamedev", Access.WRITE,
        "Places explicit tiles: cells [[x, y, tile], …] (tile null = empty), rows of space-separated tile indices ('.' = empty) starting at x/y, or a single x/y/tile.",
        linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "cells" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "[[x, y, tile], …]."),
        "rows" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "string"), "description" to "e.g. [\"0 1 1 2\", \". 3 3 .\"]."),
        "x" to X, "y" to Y, "tile" to integer("Tile index.", 0)))
    tool("tilemap_terrain", "Paint terrain (autotile)", "gamedev", Access.WRITE,
        "Paints terrain on a blob-tileset map and re-autotiles every cell (edges, inner/outer corners chosen automatically). Give ASCII rows ('#' = terrain, '.' = empty), a rectangle x/y/width/height, or cells [[x, y], …]; mode remove erases.",
        linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "rows" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "string"), "description" to "ASCII terrain rows."),
        "replace" to boolean("With rows: '.' clears terrain (default true)."), "x" to X, "y" to Y, "width" to integer("Rect width.", 1), "height" to integer("Rect height.", 1),
        "cells" to linkedMapOf<String, Any?>("type" to "array", "items" to linkedMapOf<String, Any?>("type" to "array"), "description" to "[[x, y], …]."), "mode" to string("add (default) or remove.", listOf("add", "remove"))))
    tool("tilemap_bake", "Bake tilemap to layer", "gamedev", Access.WRITE, "Draws a tilemap into a layer cel at x/y (so it composites, lights and exports with the scene).",
        props(linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "x" to X, "y" to Y, "replace" to boolean("Also clear pixels under empty cells.")), target()))
    tool("gamedev_inspect", "Inspect game data", "gamedev", Access.READ, "Lists slices (with keys/pivots), tilesets, tilemaps (with ASCII terrain) and layer blend modes.")
    tool("tilemap_render", "Look at tilemap", "gamedev", Access.READ, "Renders a tilemap to PNG.", linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "scale" to integer("Upscale 1-64.", 1, 64)), output = Output.PNG_IMAGE)
    tool("export_tileset", "Tileset image", "gamedev", Access.READ, "Renders a tileset atlas PNG (blob sets: 8 columns in mask order, masks listed).",
        linkedMapOf<String, Map<String, Any?>>("tileset" to tilesetRef, "columns" to integer("Tiles per row.", 1, 256), "scale" to integer("Upscale 1-64.", 1, 64)), output = Output.PNG_IMAGE)
    tool("render_lit", "Preview 2D lighting", "gamedev", Access.READ,
        "Renders a frame with normal-mapped 2D point lights (torches, campfires, spells) — what Godot/Unity 2D lights do with the exported normal map. Normals are generated from the art or taken from a hand-painted normal layer; unlit_layers (fire, glowing eyes, UI) stay full-bright. bands ≥ 2 quantizes light into crisp pixel-art steps.",
        linkedMapOf<String, Map<String, Any?>>("frame" to FRAME,
        "lights" to linkedMapOf<String, Any?>("type" to "array", "description" to "Point lights.", "items" to linkedMapOf<String, Any?>("type" to "object", "properties" to linkedMapOf<String, Map<String, Any?>>(
            "x" to num("X."), "y" to num("Y."), "z" to num("Height above the sprites (default 24)."), "color" to color("Light color."), "intensity" to num("Default 1.5."), "radius" to num("Reach in px (default 64).")))),
        "ambient" to color("Ambient light color (default dim blue night)."), "bands" to integer("Light steps (0 = smooth).", 0, 32),
        "normal_strength" to num("Normal map influence 0-8 (default 1)."), "normal_layer" to LAYER, "unlit_layers" to linkedMapOf<String, Any?>("type" to "array", "items" to LAYER, "description" to "Layers drawn full-bright on top."),
        "strength" to num("Generated normal bump strength."), "bevel" to num("Generated normal bevel px."), "background" to color("Opaque background."), "scale" to integer("Upscale 1-64.", 1, 64)),
        output = Output.PNG_IMAGE)
    tool("export_atlas", "Pack texture atlas", "gamedev", Access.READ,
        "Packs frames into a trimmed, de-duplicated MaxRects texture atlas and returns it with Aseprite-compatible JSON (frames, frameTags, layers, slices) that Phaser, PixiJS, Unity and Godot importers read. Use project_export format=atlas to write the files.",
        pack, output = Output.PNG_IMAGE)
    tool("export_godot_spriteframes", "Godot SpriteFrames", "gamedev", Access.READ,
        "Packs frames into an atlas and returns a Godot 4 SpriteFrames .tres (one animation per tag, trimmed frames restored with AtlasTexture margins, per-frame durations).",
        props(pack, linkedMapOf<String, Map<String, Any?>>("texture_path" to string("res:// path of the atlas PNG (default res://atlas.png)."))), output = Output.PNG_IMAGE)
    tool("export_tiled", "Tiled map export", "gamedev", Access.READ,
        "Exports a tilemap as a Tiled TMJ map (with an embedded tileset and, for blob sets, a Wang terrain set) plus the tileset PNG.",
        linkedMapOf<String, Map<String, Any?>>("tilemap" to tilemapRef, "image" to string("Tileset image file name referenced by the map.")), output = Output.PNG_IMAGE)

}
