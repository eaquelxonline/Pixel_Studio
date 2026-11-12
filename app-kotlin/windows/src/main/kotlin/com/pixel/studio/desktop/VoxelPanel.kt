package com.pixel.studio.desktop

import com.pixel.studio.json.Json
import com.pixel.studio.json.list
import com.pixel.studio.json.str
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Cursor
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.GridLayout
import java.awt.RenderingHints
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import java.awt.image.BufferedImage
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.imageio.ImageIO
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.ButtonGroup
import javax.swing.JButton
import javax.swing.JCheckBox
import javax.swing.JColorChooser
import javax.swing.JComboBox
import javax.swing.JFileChooser
import javax.swing.JLabel
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JSlider
import javax.swing.JSpinner
import javax.swing.JTextField
import javax.swing.JToggleButton
import javax.swing.SpinnerNumberModel
import javax.swing.SwingUtilities
import javax.swing.Timer

/**
 * The 3D pixel workspace: an orbit viewport rendered by the Rust voxel engine (PBR, AO,
 * outlines), click-to-sculpt tools, materials, bones, animation playback and 2D sprite baking.
 */
class VoxelPanel(private val c: EditorController) : JPanel(BorderLayout()) {
    enum class Mode(val label: String) { ORBIT("Orbit"), ADD("Add"), PAINT("Paint"), ERASE("Erase"), PICK("Pick color") }

    private var mode = Mode.ORBIT
    private var yaw = 25.0
    private var pitch = 20.0
    private var scale = 0.0 // 0 = not fitted yet
    private var focus = doubleArrayOf(256.0, 150.0, 256.0)
    private var color = "#C7303A"
    private var playing: Timer? = null
    private var frame = 0.0

    private val viewport = Viewport()
    private val model = JComboBox<String>()
    private val material = JComboBox<String>()
    private val bone = JComboBox<String>()
    private val animation = JComboBox<String>()
    private val pass = JComboBox(arrayOf("lit", "albedo", "normal", "occlusion", "roughness", "metallic", "emission", "depth"))
    private val brush = JSpinner(SpinnerNumberModel(1, 1, 40, 1))
    private val mirror = JCheckBox("Mirror X")
    private val outline = JCheckBox("Outline", true)
    private val smooth = JSlider(0, 100, 0)
    private val shadows = JCheckBox("Shadows", true)
    private val glow = JCheckBox("Bloom", true)
    private val toon = JSpinner(SpinnerNumberModel(0, 0, 8, 1))
    private val hex = JTextField(color, 8)
    private val status = JLabel(" ")
    private var updatingLists = false

    private val renderer = Executors.newSingleThreadExecutor { r -> Thread(r, "voxel-render").apply { isDaemon = true } }
    private val pending = AtomicBoolean(false)
    @Volatile private var image: BufferedImage? = null

    private val swatch: JPanel = object : JPanel() {
        init { preferredSize = Dimension(36, 26); toolTipText = "Any of 16.7M colors — click to choose" }
        override fun paintComponent(g: Graphics) { g.color = Theme.parse(color); g.fillRect(0, 0, width, height) }
    }

    init {
        background = Theme.BG
        add(viewport, BorderLayout.CENTER)
        add(JScrollPane(controls()).apply { preferredSize = Dimension(280, 600); border = BorderFactory.createEmptyBorder() }, BorderLayout.EAST)
        add(status.apply { foreground = Theme.TEXT_DIM; border = BorderFactory.createEmptyBorder(3, 10, 3, 10) }, BorderLayout.SOUTH)
        c.onChange { refreshLists(); requestRender() }
        refreshLists()
    }

    // ---- engine helpers -------------------------------------------------------------------

    private fun modelArg(): Map<String, Any?> = model.selectedItem?.let { mapOf("model" to it) } ?: emptyMap()

    private fun cameraArgs(w: Int, h: Int): Map<String, Any?> {
        val a = LinkedHashMap<String, Any?>(modelArg())
        a["yaw"] = yaw
        a["pitch"] = pitch
        a["width"] = w.coerceIn(8, 1600)
        a["height"] = h.coerceIn(8, 1600)
        if (scale > 0) {
            a["scale"] = scale
            a["focus"] = focus.toList()
        }
        return a
    }

    private fun hasModel() = model.itemCount > 0

    fun refreshLists() {
        val info = c.engine.execute("voxel_inspect").result
        updatingLists = true
        val models = info.list("models").orEmpty().map { (it as Map<*, *>)["name"] as String }
        val selected = model.selectedItem
        model.removeAllItems(); models.forEach { model.addItem(it) }
        if (selected != null && selected in models) model.selectedItem = selected
        val m = info.list("models").orEmpty().map { it as Map<*, *> }.firstOrNull { it["name"] == model.selectedItem }
        val keepMat = material.selectedItem
        material.removeAllItems(); info.list("materials").orEmpty().forEach { material.addItem((it as Map<*, *>)["name"] as String) }
        if (keepMat != null) material.selectedItem = keepMat
        val detail = if (m != null) c.engine.execute("voxel_inspect", mapOf("model" to m["name"])).result.list("models").orEmpty().map { it as Map<*, *> }.firstOrNull { it["name"] == m["name"] } else null
        val keepBone = bone.selectedItem
        bone.removeAllItems(); bone.addItem("(auto)")
        (detail?.get("skeleton") as? List<*>).orEmpty().forEach { bone.addItem((it as Map<*, *>)["name"] as String) }
        if (keepBone != null) bone.selectedItem = keepBone
        val keepAnim = animation.selectedItem
        animation.removeAllItems(); animation.addItem("(rest pose)")
        (m?.get("animations") as? List<*>).orEmpty().forEach { animation.addItem((it as Map<*, *>)["name"] as String) }
        if (keepAnim != null) animation.selectedItem = keepAnim
        updatingLists = false
        if (m != null && scale == 0.0) {
            val size = (m["size"] as List<*>).map { (it as Long).toDouble() }
            focus = doubleArrayOf(size[0] / 2, size[1] * 0.42, size[2] / 2)
        }
    }

    fun requestRender() {
        if (!pending.compareAndSet(false, true)) return
        renderer.execute {
            pending.set(false)
            if (!hasModel() || viewport.width < 16) {
                image = null
                SwingUtilities.invokeLater { viewport.repaint() }
                return@execute
            }
            val args = LinkedHashMap(cameraArgs(viewport.width, viewport.height))
            args["pass"] = pass.selectedItem
            args["smoothness"] = smooth.value / 100.0
            args["outline"] = outline.isSelected
            args["toon_steps"] = toon.value
            args["inner_lines"] = false
            args["background_gradient"] = listOf("#3A332D", "#0E0D13")
            if (shadows.isSelected) { args["shadows"] = 1; args["ground_shadow"] = 0.35 }
            if (glow.isSelected) args["bloom"] = 0.6
            animName()?.let { args["animation"] = it; args["frame"] = frame }
            val t = System.nanoTime()
            val r = c.engine.execute("voxel_render", args)
            if (r.ok) {
                if (scale == 0.0) scale = (r.result["scale"] as? Number)?.toDouble() ?: 1.0
                image = ImageIO.read(r.binary.inputStream())
                val ms = (System.nanoTime() - t) / 1_000_000
                SwingUtilities.invokeLater {
                    status.text = "${model.selectedItem} • yaw ${yaw.toInt()}° pitch ${pitch.toInt()}° • ${"%.2f".format(scale)} px/voxel • ${r.result["faces"]} faces • $ms ms"
                    viewport.repaint()
                }
            } else {
                SwingUtilities.invokeLater { status.text = "⚠ ${r.errorMessage}" }
            }
        }
    }

    private fun animName(): String? = (animation.selectedItem as? String)?.takeIf { !it.startsWith("(") }

    private fun edit(px: Int, py: Int) {
        if (!hasModel()) return
        val hit = c.engine.execute("voxel_pick", cameraArgs(viewport.width, viewport.height) + mapOf("px" to px, "py" to py)).result
        if (hit["hit"] != true) return
        val v = (hit["voxel"] as List<*>).map { (it as Long).toInt() }
        val n = (hit["normal"] as List<*>).map { (it as Long).toInt() }
        if (mode == Mode.PICK) {
            hit.str("color")?.let { setColor(it) }
            return
        }
        val target = if (mode == Mode.ADD) listOf(v[0] + n[0], v[1] + n[1], v[2] + n[2]) else v
        val r = (brush.value as Int)
        val args = LinkedHashMap<String, Any?>(modelArg())
        args["shape"] = "ellipsoid"
        args["center"] = target.map { it + 0.5 }
        args["radius"] = r - 0.5 + 0.01
        args["mode"] = when (mode) { Mode.ERASE -> "erase"; Mode.PAINT -> "paint"; else -> "add" }
        if (mode != Mode.ERASE) args["color"] = color
        material.selectedItem?.let { if (mode != Mode.ERASE) args["material"] = it }
        (bone.selectedItem as? String)?.takeIf { !it.startsWith("(") }?.let { args["bone"] = it }
        if (mirror.isSelected) args["mirror_x"] = true
        c.run("voxel_shape", args)
    }

    private fun setColor(h: String) {
        color = h
        hex.text = h
        swatch.repaint()
    }

    // ---- UI -------------------------------------------------------------------------------

    private fun row(vararg items: java.awt.Component) = JPanel(FlowLayout(FlowLayout.LEFT, 4, 2)).apply { isOpaque = false; items.forEach { add(it) } }

    private fun label(t: String) = JLabel(t).apply { foreground = Theme.TEXT_DIM }

    private fun section(title: String, vararg rows: java.awt.Component) = JPanel().apply {
        layout = BoxLayout(this, BoxLayout.Y_AXIS)
        background = Theme.PANEL
        border = Theme.section(title)
        rows.forEach { add(it) }
    }

    private fun controls(): JPanel {
        val tools = JPanel(GridLayout(0, 3, 2, 2)).apply { isOpaque = false }
        val group = ButtonGroup()
        for (m in Mode.entries) {
            val b = JToggleButton(m.label).apply { isFocusable = false; isSelected = m == mode; addActionListener { mode = m; viewport.cursor = Cursor.getPredefinedCursor(if (m == Mode.ORBIT) Cursor.MOVE_CURSOR else Cursor.CROSSHAIR_CURSOR) } }
            group.add(b); tools.add(b)
        }
        swatch.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                JColorChooser.showDialog(this@VoxelPanel, "Voxel color", Theme.parse(color))?.let { setColor(Theme.hex(it)) }
            }
        })
        hex.addActionListener { if (hex.text.matches(Regex("#?[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?"))) setColor(if (hex.text.startsWith("#")) hex.text.uppercase() else "#" + hex.text.uppercase()) }
        val rerender = { _: Any? -> if (!updatingLists) requestRender() }
        listOf(model, pass, animation).forEach { it.addActionListener { _ -> if (!updatingLists) { if (it == model) { scale = 0.0; refreshLists() }; requestRender() } } }
        outline.addActionListener(rerender); smooth.addChangeListener { if (!smooth.valueIsAdjusting) requestRender() }; toon.addChangeListener(rerender)
        listOf(outline, mirror, shadows, glow).forEach { it.isOpaque = false; it.foreground = Theme.TEXT }
        shadows.addActionListener(rerender); glow.addActionListener(rerender)
        val frameSlider = JSlider(0, 70, 0).apply { isOpaque = false; addChangeListener { frame = value / 10.0; requestRender() } }

        return JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            background = Theme.PANEL
            add(section("Model",
                row(model),
                row(Theme.button("New…") { newModel() }, Theme.button("Load knight demo") { loadHero() }, Theme.button("Fit") { scale = 0.0; refreshLists(); requestRender() }),
            ))
            add(section("Sculpt", tools, row(label("Brush"), brush, mirror), row(swatch, hex), row(label("Material"), material), row(label("Bone"), bone)))
            add(section("Materials",
                row(Theme.button("Generate PBR texture…") { generateTexture() }),
                row(Theme.button("Material settings…") { editMaterial() }),
            ))
            add(section("Look", row(label("Pass"), pass), row(label("Smooth"), smooth), row(outline, shadows, glow), row(label("Toon"), toon)))
            add(section("Rig & animation",
                row(label("Anim"), animation),
                row(Theme.button("▶ / ■") { togglePlay(frameSlider) }, frameSlider),
                row(Theme.button("Walk") { generate("walk") }, Theme.button("Run") { generate("run") }, Theme.button("Idle") { generate("idle") }),
                row(Theme.button("Auto-bind voxels") { c.run("rig_bind", modelArg() + mapOf("only_unbound" to true)) }),
            ))
            add(section("Sprites",
                row(Theme.button("Bake 8 directions → 2D…") { bake() }),
                row(Theme.button("Export sprite sheet…") { exportSheet() }),
            ))
        }
    }

    private fun newModel() {
        val name = JOptionPane.showInputDialog(this, "Model name", "Model ${model.itemCount + 1}") ?: return
        val size = JOptionPane.showInputDialog(this, "Size (width x height x depth, max 512)", "64x64x64") ?: return
        val d = size.lowercase().split('x').mapNotNull { it.trim().toIntOrNull() }
        if (d.size != 3) return
        val r = c.run("voxel_model_create", mapOf("name" to name, "width" to d[0], "height" to d[1], "depth" to d[2]))
        if (r.ok) {
            c.run("voxel_shape", mapOf("model" to name, "shape" to "box", "min" to listOf(d[0] / 2 - 2, 0, d[2] / 2 - 2), "max" to listOf(d[0] / 2 + 1, 3, d[2] / 2 + 1), "color" to "#7A7A86"))
            scale = 0.0
            refreshLists()
            model.selectedItem = name
        }
    }

    private fun loadHero() {
        val script = javaClass.getResourceAsStream("knight.jsonl")?.bufferedReader()?.readLines() ?: return
        if (c.engine.execute("voxel_inspect").result.list("models").orEmpty().any { (it as Map<*, *>)["name"] == "Knight" }) {
            model.selectedItem = "Knight"; return
        }
        status.text = "Building Sir Valen (${script.size} commands)…"
        renderer.execute {
            // One transaction-like burst; each line is an ordinary, undoable engine command.
            script.filter { it.isNotBlank() }.forEach { c.engine.executeRaw(it.toByteArray()) }
            SwingUtilities.invokeLater { scale = 0.0; model.selectedItem = "Diorama"; refreshLists(); requestRender() }
        }
    }

    private fun generateTexture() {
        val kinds = arrayOf("brushed_metal", "hammered_metal", "leather", "fabric", "scales", "stone", "wood", "chainmail", "noise")
        val kind = JOptionPane.showInputDialog(this, "Surface", "Generate PBR texture set", JOptionPane.PLAIN_MESSAGE, null, kinds, kinds[0]) as String? ?: return
        val name = JOptionPane.showInputDialog(this, "Material name", kind) ?: return
        val metal = kind.contains("metal") || kind == "chainmail"
        c.run("texture_generate", mapOf("name" to name, "kind" to kind, "size" to 64, "metallic" to if (metal) 0.9 else 0.0, "roughness" to if (metal) 0.3 else 0.7))
        refreshLists()
        material.selectedItem = name
    }

    private fun editMaterial() {
        val name = material.selectedItem as? String ?: return
        val info = c.engine.execute("voxel_inspect").result.list("materials").orEmpty().map { it as Map<*, *> }.firstOrNull { it["name"] == name } ?: return
        fun slider(v: Any?, max: Int = 100) = JSlider(0, max, (((v as? Number)?.toDouble() ?: 0.0) * 100).toInt().coerceIn(0, max))
        val rough = slider(info["roughness"]); val metal = slider(info["metallic"]); val emit = slider(info["emission"], 500)
        val p = JPanel(GridLayout(0, 2)).apply { add(JLabel("Roughness")); add(rough); add(JLabel("Metallic")); add(metal); add(JLabel("Emission")); add(emit) }
        if (JOptionPane.showConfirmDialog(this, p, "PBR material: $name", JOptionPane.OK_CANCEL_OPTION) == JOptionPane.OK_OPTION) {
            c.run("material_set", mapOf("name" to name, "roughness" to rough.value / 100.0, "metallic" to metal.value / 100.0, "emission" to emit.value / 100.0))
        }
    }

    private fun generate(kind: String) {
        val r = c.run("anim3d_generate", modelArg() + mapOf("kind" to kind, "frames" to 8))
        if (!r.ok) JOptionPane.showMessageDialog(this, r.errorMessage) else { refreshLists(); animation.selectedItem = kind }
    }

    private fun togglePlay(slider: JSlider) {
        playing?.let { it.stop(); playing = null; return }
        val t = Timer(110) { slider.value = (slider.value + 10) % 80 }
        playing = t
        t.start()
    }

    private fun bake() {
        val size = JOptionPane.showInputDialog(this, "Sprite size (px)", "256")?.toIntOrNull() ?: return
        val args = LinkedHashMap<String, Any?>(modelArg())
        animName()?.let { args["animation"] = it }
        args += mapOf("directions" to 8, "width" to size, "height" to size, "pitch" to 28, "supersample" to 2, "outline" to true, "smoothness" to smooth.value / 100.0,
            "shadows" to 1, "ground_shadow" to 0.38, "bloom" to 0.55, "bloom_radius" to 4)
        status.text = "Baking 8 directions…"
        val r = c.run("voxel_render_to_frames", args)
        status.text = if (r.ok) "Baked ${r.result["framesAdded"]} frames into the 2D tab (tags ${r.result["tags"]})" else "⚠ ${r.errorMessage}"
    }

    private fun exportSheet() {
        val ch = JFileChooser(workspaceDir()).apply { selectedFile = File("${model.selectedItem}_${animName() ?: "pose"}_${pass.selectedItem}.png") }
        if (ch.showSaveDialog(this) != JFileChooser.APPROVE_OPTION) return
        val args = LinkedHashMap<String, Any?>(modelArg())
        animName()?.let { args["animation"] = it }
        args += mapOf("directions" to 8, "width" to 256, "pitch" to 28, "supersample" to 2, "outline" to (pass.selectedItem == "lit"), "pass" to pass.selectedItem, "smoothness" to smooth.value / 100.0,
            "shadows" to 1, "ground_shadow" to if (pass.selectedItem == "lit") 0.38 else 0, "bloom" to if (pass.selectedItem == "lit") 0.55 else 0)
        val r = c.engine.execute("voxel_render_sheet", args)
        if (r.ok) {
            ch.selectedFile.writeBytes(r.binary)
            File(ch.selectedFile.path.removeSuffix(".png") + ".json").writeText(Json.write(r.result))
        } else {
            JOptionPane.showMessageDialog(this, r.errorMessage)
        }
    }

    private inner class Viewport : JPanel() {
        private var last: java.awt.Point? = null
        private var button = 0

        init {
            background = Theme.CANVAS_BG
            cursor = Cursor.getPredefinedCursor(Cursor.MOVE_CURSOR)
            val m = object : MouseAdapter() {
                override fun mousePressed(e: MouseEvent) {
                    last = e.point
                    button = e.button
                    if (mode != Mode.ORBIT && SwingUtilities.isLeftMouseButton(e)) edit(e.x, e.y)
                }
                override fun mouseDragged(e: MouseEvent) {
                    val l = last ?: return
                    val (dx, dy) = (e.x - l.x) to (e.y - l.y)
                    last = e.point
                    if (mode == Mode.ORBIT && button == MouseEvent.BUTTON1) {
                        yaw = (yaw + dx * 0.6) % 360
                        pitch = (pitch + dy * 0.4).coerceIn(-85.0, 85.0)
                    } else if (button != MouseEvent.BUTTON1 && scale > 0) {
                        // Pan: move the focus point in the view plane.
                        val ry = Math.toRadians(yaw)
                        focus[0] -= dx / scale * Math.cos(ry)
                        focus[2] += dx / scale * Math.sin(ry)
                        focus[1] += dy / scale
                    } else if (mode != Mode.ORBIT) {
                        edit(e.x, e.y)
                        return
                    }
                    requestRender()
                }
                override fun mouseWheelMoved(e: MouseWheelEvent) {
                    if (scale > 0) {
                        scale = (scale * if (e.wheelRotation < 0) 1.15 else 1 / 1.15).coerceIn(0.1, 40.0)
                        requestRender()
                    }
                }
            }
            addMouseListener(m); addMouseMotionListener(m); addMouseWheelListener(m)
            addComponentListener(object : java.awt.event.ComponentAdapter() {
                override fun componentResized(e: java.awt.event.ComponentEvent?) = requestRender()
            })
        }

        override fun paintComponent(g0: Graphics) {
            super.paintComponent(g0)
            val g = g0 as Graphics2D
            g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
            val img = image
            if (img == null) {
                g.color = Theme.TEXT_DIM
                g.drawString(if (hasModel()) "Rendering…" else "No 3D model yet — click “New…” or “Load hero demo”.", 24, 32)
                return
            }
            g.drawImage(img, 0, 0, null)
            g.color = Color(255, 255, 255, 60)
            g.drawString("Drag: orbit • Right-drag: pan • Wheel: zoom", 12, height - 12)
        }
    }
}
