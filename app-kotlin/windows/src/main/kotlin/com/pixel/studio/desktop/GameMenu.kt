package com.pixel.studio.desktop

import com.pixel.studio.gateway.tools.DirectoryWorkspace
import com.pixel.studio.json.Json
import com.pixel.studio.json.str
import java.awt.BorderLayout
import java.awt.Font
import java.awt.GridLayout
import java.io.File
import javax.imageio.ImageIO
import javax.swing.ImageIcon
import javax.swing.JComboBox
import javax.swing.JFileChooser
import javax.swing.JFrame
import javax.swing.JLabel
import javax.swing.JMenu
import javax.swing.JMenuItem
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JSpinner
import javax.swing.JTextArea
import javax.swing.JTextField
import javax.swing.SpinnerNumberModel

/** "Game" menu: the game-production tools (blend modes, slices, autotile levels, 2D light, engine exports). */
class GameMenu(private val owner: JFrame, private val c: EditorController, private val workspace: () -> File) : JMenu("Game") {
    init {
        item("Layer blend mode…") { blendMode() }
        item("Add slice (hitbox / pivot)…") { slice() }
        item("Palette swap current color…") { paletteSwap() }
        item("RotSprite rotate layer…") { rotate() }
        addSeparator()
        item("Autotile level from ASCII…") { autotileLevel() }
        item("Preview 2D lighting…") { lightPreview() }
        addSeparator()
        item("Export atlas + Aseprite JSON…") { exportPair("export_atlas", "json") { r, base -> Json.write(r.result["data"]) } }
        item("Export Godot SpriteFrames…") { exportPair("export_godot_spriteframes", "tres") { r, _ -> r.result.str("tres") ?: "" } }
        item("Export Tiled map…") { exportTiled() }
    }

    private fun item(text: String, f: () -> Unit) = add(JMenuItem(text).apply { addActionListener { f() } })

    private fun form(title: String, vararg rows: Pair<String, java.awt.Component>): Boolean {
        val p = JPanel(GridLayout(rows.size, 2, 6, 4))
        rows.forEach { (label, comp) -> p.add(JLabel(label)); p.add(comp) }
        return JOptionPane.showConfirmDialog(owner, p, title, JOptionPane.OK_CANCEL_OPTION) == JOptionPane.OK_OPTION
    }

    private fun spinner(v: Int, min: Int, max: Int) = JSpinner(SpinnerNumberModel(v, min, max, 1))

    private fun report(r: com.pixel.studio.bridge.EngineResponse) {
        if (!r.ok) JOptionPane.showMessageDialog(owner, r.errorMessage ?: "failed", "Pixel Studio", JOptionPane.WARNING_MESSAGE)
    }

    private fun blendMode() {
        val modes = arrayOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light")
        val current = c.doc.layers.getOrNull(c.doc.activeLayer)?.blend ?: "normal"
        val box = JComboBox(modes).apply { selectedItem = current }
        if (form("Blend mode of the active layer", "Mode" to box)) report(c.run("layer_blend", mapOf("mode" to box.selectedItem)))
    }

    private fun slice() {
        val name = JComboBox(arrayOf("hitbox", "hurtbox", "pivot", "attach_weapon", "interact")).apply { isEditable = true }
        val x = spinner(0, -4096, 4096); val y = spinner(0, -4096, 4096)
        val w = spinner(c.doc.width / 2, 1, 4096); val h = spinner(c.doc.height / 2, 1, 4096)
        val px = spinner(-1, -1, 4096); val py = spinner(-1, -1, 4096)
        if (!form("Slice on the current frame", "Name" to name, "X" to x, "Y" to y, "Width" to w, "Height" to h, "Pivot X (-1 = none)" to px, "Pivot Y" to py)) return
        val args = linkedMapOf<String, Any?>("name" to name.selectedItem.toString(), "frame" to c.doc.activeFrame, "x" to x.value, "y" to y.value, "width" to w.value, "height" to h.value)
        if ((px.value as Int) >= 0 && (py.value as Int) >= 0) args["pivot"] = listOf(px.value, py.value)
        report(c.run("slice_set", args))
    }

    private fun paletteSwap() {
        val to = JTextField("#3070C0")
        val scope = JComboBox(arrayOf("layer", "cel", "frame", "all"))
        if (form("Replace ${c.color} everywhere in…", "New color" to to, "Scope" to scope)) {
            report(c.run("palette_swap", mapOf("map" to mapOf(c.color to to.text.trim()), "scope" to scope.selectedItem)))
        }
    }

    private fun rotate() {
        val deg = spinner(45, -360, 360)
        if (form("RotSprite (clean pixel-art rotation)", "Degrees (clockwise)" to deg)) report(c.run("rotsprite", mapOf("degrees" to deg.value)))
    }

    private fun autotileLevel() {
        val cols = c.doc.width / 16; val rows = c.doc.height / 16
        val text = JTextArea((0 until rows).joinToString("\n") { r -> if (r < rows / 2) ".".repeat(cols) else "#".repeat(cols) }, rows.coerceIn(4, 16), cols.coerceIn(8, 40))
        text.font = Font(Font.MONOSPACED, Font.PLAIN, 13)
        val fill = JTextField(c.color)
        val tile = spinner(16, 4, 64)
        val lip = spinner(4, 0, 32)
        val p = JPanel(BorderLayout(6, 6))
        p.add(JLabel("Terrain rows: '#' = ground, '.' = empty (one character per tile)"), BorderLayout.NORTH)
        p.add(JScrollPane(text), BorderLayout.CENTER)
        p.add(JPanel(GridLayout(3, 2, 6, 4)).apply {
            add(JLabel("Fill color")); add(fill); add(JLabel("Tile size")); add(tile); add(JLabel("Cliff lip (px)")); add(lip)
        }, BorderLayout.SOUTH)
        if (JOptionPane.showConfirmDialog(owner, p, "Autotile level (47-tile blob set)", JOptionPane.OK_CANCEL_OPTION) != JOptionPane.OK_OPTION) return
        val t = tile.value as Int
        val lines = text.text.lines().filter { it.isNotBlank() }
        val name = "terrain_" + System.currentTimeMillis() % 100000
        val r = c.run("transaction", mapOf("commands" to listOf(
            mapOf("op" to "tileset_generate_blob", "name" to name, "tile_size" to t, "fill" to fill.text.trim(), "lip" to lip.value),
            mapOf("op" to "tilemap_create", "name" to name, "tileset" to name, "columns" to (c.doc.width / t).coerceAtLeast(1), "rows" to (c.doc.height / t).coerceAtLeast(1)),
            mapOf("op" to "tilemap_terrain", "tilemap" to name, "rows" to lines),
            mapOf("op" to "tilemap_bake", "tilemap" to name),
        )))
        report(r)
    }

    private fun lightPreview() {
        val x = spinner(c.doc.width / 2, -4096, 4096); val y = spinner(c.doc.height / 2, -4096, 4096)
        val color = JTextField("#FFB060")
        val radius = spinner(maxOf(c.doc.width, c.doc.height) / 2, 1, 8192)
        val bands = spinner(4, 0, 32)
        val ambient = JTextField("#3E4A7C")
        if (!form("Normal-mapped 2D light", "Light X" to x, "Light Y" to y, "Color" to color, "Radius" to radius, "Bands (0 = smooth)" to bands, "Ambient" to ambient)) return
        val scale = (720 / maxOf(c.doc.width, c.doc.height)).coerceIn(1, 16)
        val r = c.run("render_lit", mapOf("frame" to c.doc.activeFrame, "scale" to scale, "bands" to bands.value, "ambient" to ambient.text.trim(),
            "lights" to listOf(mapOf("x" to x.value, "y" to y.value, "color" to color.text.trim(), "radius" to radius.value, "intensity" to 2.5))))
        if (!r.ok) return report(r)
        val img = ImageIO.read(r.binary.inputStream())
        JOptionPane.showMessageDialog(owner, JLabel(ImageIcon(img)), "2D lighting preview", JOptionPane.PLAIN_MESSAGE)
    }

    private fun chooser(base: String): File? {
        val ch = JFileChooser(c.file?.parentFile ?: workspace())
        ch.selectedFile = File(DirectoryWorkspace.sanitize(base) + ".png")
        return if (ch.showSaveDialog(owner) == JFileChooser.APPROVE_OPTION) ch.selectedFile.let { if (it.name.endsWith(".png")) it else File(it.path + ".png") } else null
    }

    private fun exportPair(op: String, ext: String, doc: (com.pixel.studio.bridge.EngineResponse, String) -> String) {
        val png = chooser(c.doc.name) ?: return
        val base = png.name.removeSuffix(".png")
        val r = c.run(op, mapOf("image" to png.name, "texture_path" to "res://${png.name}"))
        if (!r.ok) return report(r)
        png.writeBytes(r.binary)
        File(png.parentFile, "$base.$ext").writeText(doc(r, base))
    }

    private fun exportTiled() {
        val png = chooser(c.doc.name + "_tileset") ?: return
        val r = c.run("export_tiled", mapOf("image" to png.name))
        if (!r.ok) return report(r)
        png.writeBytes(r.binary)
        File(png.parentFile, png.name.removeSuffix("_tileset.png").removeSuffix(".png") + ".tmj").writeText(Json.write(r.result["tmj"]))
    }
}
