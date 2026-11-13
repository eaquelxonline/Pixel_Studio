// Pixel Studio — desktop editor (Windows / Linux / macOS, JDK Swing): 2D pixel and 3D voxel workspaces.

package com.pixel.studio.desktop

import com.pixel.studio.gateway.AgentGateway
import com.pixel.studio.gateway.Confirmer
import com.pixel.studio.gateway.DirectoryWorkspace
import com.pixel.studio.gateway.EngineResponse
import com.pixel.studio.gateway.GatewayConfig
import com.pixel.studio.gateway.Json
import com.pixel.studio.gateway.PixelEngine
import com.pixel.studio.gateway.bool
import com.pixel.studio.gateway.list
import com.pixel.studio.gateway.long
import com.pixel.studio.gateway.obj
import com.pixel.studio.gateway.str
import java.awt.BasicStroke
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Cursor
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Font
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.GridLayout
import java.awt.Image
import java.awt.Point
import java.awt.RenderingHints
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import java.awt.event.KeyEvent
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.awt.image.BufferedImage
import java.io.File
import java.net.BindException
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.imageio.ImageIO
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.ButtonGroup
import javax.swing.DefaultListModel
import javax.swing.ImageIcon
import javax.swing.JButton
import javax.swing.JCheckBox
import javax.swing.JColorChooser
import javax.swing.JComboBox
import javax.swing.JDialog
import javax.swing.JFileChooser
import javax.swing.JFrame
import javax.swing.JLabel
import javax.swing.JList
import javax.swing.JMenu
import javax.swing.JMenuBar
import javax.swing.JMenuItem
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JSlider
import javax.swing.JSpinner
import javax.swing.JTextArea
import javax.swing.JTextField
import javax.swing.JToggleButton
import javax.swing.KeyStroke
import javax.swing.ListCellRenderer
import javax.swing.ListSelectionModel
import javax.swing.SpinnerNumberModel
import javax.swing.SwingUtilities
import javax.swing.Timer
import javax.swing.UIManager
import javax.swing.border.Border
import javax.swing.filechooser.FileNameExtensionFilter
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

// ═══ PixelStudioApp ══════════════════════════════════════════════════════════════════════════════

const val APP_ID = "com.pixel.studio"

class PixelStudioWindow(private val c: EditorController, private val gateway: AgentGateway) : JFrame("Pixel Studio") {
    private val canvas = CanvasView(c)
    val voxels = VoxelPanel(c)
    private val status = JLabel(" ")
    private var cursorText = ""

    init {
        defaultCloseOperation = DO_NOTHING_ON_CLOSE
        iconImages = listOf("icon-32.png", "icon-64.png", "icon-256.png").mapNotNull { name ->
            javaClass.getResource("/$name")?.let { ImageIO.read(it) as Image }
        }
        jMenuBar = menus()
        contentPane.background = Theme.BG
        contentPane.layout = BorderLayout()
        val pixel2d = JPanel(BorderLayout())
        pixel2d.add(toolbar(), BorderLayout.WEST)
        pixel2d.add(canvas, BorderLayout.CENTER)
        val tabs = javax.swing.JTabbedPane()
        tabs.addTab("2D Pixel", pixel2d)
        tabs.addTab("3D Voxel", voxels)
        contentPane.add(tabs, BorderLayout.CENTER)
        pixel2d.add(JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            background = Theme.PANEL
            preferredSize = Dimension(260, 400)
            add(PalettePanel(c))
            add(LayersPanel(c))
            add(ActivityPanel(c))
        }, BorderLayout.EAST)
        contentPane.add(JPanel(BorderLayout()).apply {
            add(TimelinePanel(c, canvas), BorderLayout.CENTER)
            add(status.apply {
                border = BorderFactory.createEmptyBorder(3, 10, 3, 10)
                foreground = Theme.TEXT_DIM
                isOpaque = true
                background = Theme.BG
            }, BorderLayout.SOUTH)
        }, BorderLayout.SOUTH)
        canvas.statusListener = { cursorText = it; updateStatus() }
        c.onChange { updateStatus() }
        updateStatus()
        addWindowListener(object : WindowAdapter() {
            override fun windowClosing(e: WindowEvent) = quit()
        })
        size = Dimension(1360, 880)
        setLocationRelativeTo(null)
    }

    private fun updateStatus() {
        val d = c.doc
        val agent = if (gateway.running) "Agent Mode: ${gateway.mcpUrl}" else "Agent Mode off"
        val err = c.lastError?.let { "  ⚠ $it" } ?: ""
        status.text = "${d.name} • ${d.width}×${d.height} • frame ${d.activeFrame + 1}/${d.frames.size} • rev ${d.revision} • $agent   $cursorText$err"
        title = "Pixel Studio — ${d.name}" + (c.file?.let { " (${it.name})" } ?: "")
    }

    private fun toolbar(): JPanel {
        val panel = JPanel(GridLayout(0, 1, 2, 2)).apply { background = Theme.PANEL; border = BorderFactory.createEmptyBorder(6, 6, 6, 6) }
        val group = ButtonGroup()
        for (t in Tool.entries) {
            val b = JToggleButton(t.label).apply {
                isFocusable = false
                toolTipText = "${t.label} (${t.key})" + if (t == Tool.RECTANGLE || t == Tool.ELLIPSE) " — hold Shift to fill" else ""
                isSelected = t == c.tool
                addActionListener { c.tool = t }
            }
            group.add(b)
            panel.add(b)
            rootPane.inputMap.put(KeyStroke.getKeyStroke(t.key), "tool-$t")
            rootPane.actionMap.put("tool-$t", action { c.tool = t; b.isSelected = true })
        }
        panel.add(Theme.button("Zoom +") { canvas.zoomBy(1.25) })
        panel.add(Theme.button("Zoom −") { canvas.zoomBy(0.8) })
        panel.add(Theme.button("Fit") { canvas.fit() })
        panel.add(Theme.button("Grid") { c.showGrid = !c.showGrid; canvas.repaint() })
        return JPanel(BorderLayout()).apply { background = Theme.PANEL; add(panel, BorderLayout.NORTH) }
    }

    private fun action(f: () -> Unit) = object : javax.swing.AbstractAction() {
        override fun actionPerformed(e: java.awt.event.ActionEvent?) = f()
    }

    private fun item(text: String, key: KeyStroke? = null, f: () -> Unit) = JMenuItem(text).apply {
        if (key != null) accelerator = key
        addActionListener { f() }
    }

    private fun ctrl(k: Int, shift: Boolean = false) =
        KeyStroke.getKeyStroke(k, java.awt.Toolkit.getDefaultToolkit().menuShortcutKeyMaskEx or if (shift) KeyEvent.SHIFT_DOWN_MASK else 0)

    private fun menus() = JMenuBar().apply {
        add(JMenu("File").apply {
            add(item("New…", ctrl(KeyEvent.VK_N)) { newProject() })
            add(item("Open…", ctrl(KeyEvent.VK_O)) { open() })
            add(item("Save", ctrl(KeyEvent.VK_S)) { save(false) })
            add(item("Save As…", ctrl(KeyEvent.VK_S, true)) { save(true) })
            addSeparator()
            add(item("Export PNG (current frame)…") { export("png") })
            add(item("Export GIF animation…") { export("gif") })
            add(item("Export sprite sheet…") { export("spritesheet") })
            addSeparator()
            add(item("Quit", ctrl(KeyEvent.VK_Q)) { quit() })
        })
        add(JMenu("Edit").apply {
            add(item("Undo", ctrl(KeyEvent.VK_Z)) { c.run("history_undo") })
            add(item("Redo", ctrl(KeyEvent.VK_Y)) { c.run("history_redo") })
            addSeparator()
            add(item("Clear cel") { c.run("clear") })
            add(item("Flip horizontal") { c.run("flip", mapOf("axis" to "horizontal")) })
            add(item("Flip vertical") { c.run("flip", mapOf("axis" to "vertical")) })
            add(item("Resize canvas…") { resize() })
        })
        add(JMenu("Pixel Art").apply {
            add(item("Selective outline (layer)") { c.run("outline") })
            add(item("Reduce colors (frame, 16)…") {
                val n = JOptionPane.showInputDialog(this@PixelStudioWindow, "Colors", "16")?.toIntOrNull() ?: return@item
                c.run("quantize", mapOf("scope" to "frame", "colors" to n))
            })
            add(item("Hue-shifted ramp from current color") { c.run("color_ramp", mapOf("base" to c.color, "steps" to 5)) })
            addSeparator()
            val names = c.engine.execute("canvas_inspect", mapOf("detail" to "palette")).result.list("presets").orEmpty().map { (it as Map<*, *>)["name"] as String }
            add(JMenu("Load palette").apply { names.forEach { n -> add(item(n) { c.run("palette_set", mapOf("preset" to n)) }) } })
            add(item("Quantize to swatches (bayer4 dither)") { c.run("quantize", mapOf("scope" to "frame", "palette" to "swatches", "dither" to "bayer4")) })
            addSeparator()
            add(item("Export normal map…") {
                val ch = chooser("png", "PNG")
                ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + "_normal.png")
                if (ch.showSaveDialog(this@PixelStudioWindow) == JFileChooser.APPROVE_OPTION) {
                    ch.selectedFile.writeBytes(c.run("export", mapOf("format" to "normal_map", "frame" to c.doc.activeFrame)).binary)
                }
            })
        })
        add(GameMenu(this@PixelStudioWindow, c) { workspaceDir() })
        add(JMenu("Agent").apply {
            add(item("Connect an agent…", ctrl(KeyEvent.VK_K)) { AgentDialog(this@PixelStudioWindow, gateway) { toggleGateway() }.isVisible = true })
        })
    }

    private fun toggleGateway() {
        if (gateway.running) gateway.stop() else runCatching { gateway.start() }.onFailure {
            JOptionPane.showMessageDialog(this, "Could not start Agent Mode: ${it.message}")
        }
        updateStatus()
    }

    private fun sizeDialog(title: String, w: Int, h: Int): Pair<Int, Int>? {
        val ws = JSpinner(SpinnerNumberModel(w, 1, 4096, 1))
        val hs = JSpinner(SpinnerNumberModel(h, 1, 4096, 1))
        val p = JPanel(FlowLayout()).apply { add(JLabel("Width")); add(ws); add(JLabel("Height")); add(hs) }
        return if (JOptionPane.showConfirmDialog(this, p, title, JOptionPane.OK_CANCEL_OPTION) == JOptionPane.OK_OPTION) {
            Pair(ws.value as Int, hs.value as Int)
        } else null
    }

    private fun newProject() {
        val (w, h) = sizeDialog("New project", 32, 32) ?: return
        c.run("project_new", mapOf("width" to w, "height" to h, "name" to "Untitled"))
        c.file = null
        canvas.fit()
    }

    private fun resize() {
        val (w, h) = sizeDialog("Resize canvas", c.doc.width, c.doc.height) ?: return
        c.run("canvas_resize", mapOf("width" to w, "height" to h))
        canvas.fit()
    }

    private fun chooser(ext: String, desc: String) = JFileChooser(c.file?.parentFile ?: workspaceDir()).apply {
        fileFilter = FileNameExtensionFilter(desc, ext)
    }

    private fun open() {
        val ch = chooser("pxs", "Pixel Studio project (*.pxs)")
        if (ch.showOpenDialog(this) != JFileChooser.APPROVE_OPTION) return
        openFile(ch.selectedFile)
    }

    fun openFile(f: File) {
        val r = c.run("project_load", payload = f.readBytes())
        if (r.ok) { c.file = f; canvas.fit() } else JOptionPane.showMessageDialog(this, "Could not open ${f.name}: ${r.errorMessage}")
    }

    private fun save(askName: Boolean) {
        var f = c.file
        if (f == null || askName) {
            val ch = chooser("pxs", "Pixel Studio project (*.pxs)")
            ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + ".pxs")
            if (ch.showSaveDialog(this) != JFileChooser.APPROVE_OPTION) return
            f = ch.selectedFile.let { if (it.name.endsWith(".pxs")) it else File(it.path + ".pxs") }
        }
        val r = c.run("project_save")
        if (r.ok) {
            f!!.writeBytes(r.binary)
            c.file = f
            updateStatus()
        }
    }

    private fun export(format: String) {
        val ext = if (format == "gif") "gif" else "png"
        val scale = JOptionPane.showInputDialog(this, "Scale (1-64)", "1")?.toIntOrNull()?.coerceIn(1, 64) ?: return
        val ch = chooser(ext, ext.uppercase())
        ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + (if (format == "spritesheet") "_sheet." else ".") + ext)
        if (ch.showSaveDialog(this) != JFileChooser.APPROVE_OPTION) return
        val r = c.run("export", mapOf("format" to format, "scale" to scale, "frame" to c.doc.activeFrame))
        if (!r.ok) return
        ch.selectedFile.writeBytes(r.binary)
        if (format == "spritesheet") File(ch.selectedFile.path.removeSuffix(".png") + ".json").writeText(com.pixel.studio.gateway.Json.write(r.result))
    }

    private fun quit() {
        gateway.close()
        dispose()
        c.engine.close()
        System.exit(0)
    }

    /** Paints the whole window into an image (used for docs/screenshots and UI smoke tests). */
    fun snapshot(): BufferedImage {
        val img = BufferedImage(contentPane.width, contentPane.height, BufferedImage.TYPE_INT_ARGB)
        val g = img.createGraphics()
        contentPane.paint(g)
        g.dispose()
        return img
    }
}

fun workspaceDir(): File = File(System.getProperty("user.home"), "PixelStudio").apply { mkdirs() }

/** Asks the human on the Swing thread; called from gateway worker threads. */
class SwingConfirmer(private val owner: () -> JFrame?) : Confirmer {
    override fun confirm(actor: String, tool: String, summary: String): Boolean {
        var ok = false
        SwingUtilities.invokeAndWait {
            ok = JOptionPane.showConfirmDialog(owner(), "$actor wants to run a destructive action:\n\n$summary\n\nAllow it?",
                "Agent permission", JOptionPane.YES_NO_OPTION, JOptionPane.WARNING_MESSAGE) == JOptionPane.YES_OPTION
        }
        return ok
    }
}

fun main(args: Array<String>) {
    System.setProperty("awt.useSystemAAFontSettings", "on")
    val engine = PixelEngine(32, 32)
    var window: PixelStudioWindow? = null
    val workspace = DirectoryWorkspace(workspaceDir())
    var gateway = AgentGateway(engine, GatewayConfig(), workspace, SwingConfirmer { window })
    try {
        gateway.start()
    } catch (e: BindException) {
        // Another instance owns the default port: use a free one; the dialog shows the URL.
        gateway = AgentGateway(engine, GatewayConfig(port = 0), workspace, SwingConfirmer { window }).start()
    }
    val finalGateway = gateway
    SwingUtilities.invokeLater {
        Theme.install()
        val c = EditorController(engine)
        val w = PixelStudioWindow(c, finalGateway)
        window = w
        val screenshotIndex = args.indexOf("--screenshot")
        val tabIndex = args.indexOf("--tab")
        val scriptIndex = args.indexOf("--script")
        if (scriptIndex >= 0 && scriptIndex + 1 < args.size) {
            File(args[scriptIndex + 1]).readLines().filter { it.isNotBlank() }.forEach { engine.executeRaw(it.toByteArray()) }
        }
        args.firstOrNull { it.endsWith(".pxs") }?.let { w.openFile(File(it)) }
        w.isVisible = true
        if (tabIndex >= 0 && args.getOrNull(tabIndex + 1) == "3d") {
            (w.contentPane.components.firstOrNull { it is javax.swing.JTabbedPane } as? javax.swing.JTabbedPane)?.selectedIndex = 1
            if ("--hero" in args) w.voxels.javaClass.getDeclaredMethod("loadHero").apply { isAccessible = true }.invoke(w.voxels)
        }
        if (screenshotIndex >= 0 && screenshotIndex + 1 < args.size) {
            javax.swing.Timer(if ("--hero" in args) 12000 else 1500) {
                ImageIO.write(w.snapshot(), "png", File(args[screenshotIndex + 1]))
                finalGateway.close()
                System.exit(0)
            }.apply { isRepeats = false }.start()
        }
    }
}

// ═══ EditorController ════════════════════════════════════════════════════════════════════════════

enum class Tool(val label: String, val key: Char) {
    PENCIL("Pencil", 'B'), ERASER("Eraser", 'E'), LINE("Line", 'L'), RECTANGLE("Rectangle", 'R'),
    ELLIPSE("Ellipse", 'O'), FILL("Fill", 'G'), PICKER("Picker", 'I'),
}

data class LayerInfo(val index: Int, val name: String, val visible: Boolean, val locked: Boolean, val opacity: Int, val blend: String = "normal")
data class FrameInfo(val index: Int, val duration: Int)

/** Snapshot of document structure, refreshed after every revision change. */
data class DocInfo(
    val name: String = "Untitled",
    val width: Int = 32,
    val height: Int = 32,
    val revision: Long = 0,
    val layers: List<LayerInfo> = emptyList(),
    val frames: List<FrameInfo> = emptyList(),
    val activeLayer: Int = 0,
    val activeFrame: Int = 0,
    val palette: List<String> = emptyList(),
    val undo: Int = 0,
    val redo: Int = 0,
    val totalMs: Long = 0,
    val fps: Double = 0.0,
)

/**
 * UI-side controller. Every edit goes to the shared engine with actor "human"; agents edit
 * the same engine through the gateway, and both kinds of change arrive here as revision events.
 */
class EditorController(val engine: PixelEngine) {
    var tool = Tool.PENCIL
    var color = "#000000"
    var onionSkin = false
    var showGrid = true
    var file: File? = null
    @Volatile var doc = DocInfo()
        private set

    private val listeners = mutableListOf<() -> Unit>()
    private val refreshQueued = AtomicBoolean(false)

    /** Shown in the status bar; set when an engine call fails. */
    var lastError: String? = null
        private set

    init {
        engine.addChangeListener { scheduleRefresh() }
        reload()
    }

    fun onChange(l: () -> Unit) { listeners += l }

    private fun scheduleRefresh() {
        if (refreshQueued.compareAndSet(false, true)) {
            SwingUtilities.invokeLater {
                refreshQueued.set(false)
                reload()
            }
        }
    }

    fun reload() {
        val i = engine.execute("canvas_inspect").result
        val palette = engine.execute("canvas_inspect", mapOf("detail" to "palette")).result.list("colors")?.map { (it as Map<*, *>)["color"] as String } ?: emptyList()
        doc = DocInfo(
            name = i.str("name") ?: "Untitled",
            width = i.long("width")!!.toInt(),
            height = i.long("height")!!.toInt(),
            revision = i.long("revision") ?: 0,
            layers = i.list("layers")!!.map {
                @Suppress("UNCHECKED_CAST") val l = it as Map<String, Any?>
                LayerInfo(l.long("index")!!.toInt(), l.str("name")!!, l.bool("visible")!!, l.bool("locked")!!, l.long("opacity")!!.toInt(), l.str("blend") ?: "normal")
            },
            frames = i.list("frames")!!.map {
                @Suppress("UNCHECKED_CAST") val f = it as Map<String, Any?>
                FrameInfo(f.long("index")!!.toInt(), f.long("duration")!!.toInt())
            },
            activeLayer = i.long("activeLayer")!!.toInt(),
            activeFrame = i.long("activeFrame")!!.toInt(),
            palette = palette,
            undo = i.long("undoAvailable")!!.toInt(),
            redo = i.long("redoAvailable")!!.toInt(),
            totalMs = i.long("totalDurationMs") ?: 0,
            fps = (i["averageFps"] as? Number)?.toDouble() ?: 0.0,
        )
        listeners.forEach { it() }
    }

    /** Executes a human edit; returns the response and records errors for the status bar. */
    fun run(op: String, args: Map<String, Any?> = emptyMap(), payload: ByteArray? = null): EngineResponse {
        val r = engine.execute(op, args, payload = payload)
        lastError = if (r.ok) null else r.errorMessage
        if (!r.ok) listeners.forEach { it() }
        return r
    }

    fun renderFrame(frame: Int, onion: Boolean = false): BufferedImage {
        val args = linkedMapOf<String, Any?>("frame" to frame)
        if (onion) args["onion"] = mapOf("before" to 1, "after" to 1, "opacity" to 90)
        val r = engine.execute("render_frame", args)
        return toImage(r.result.long("width")?.toInt() ?: 1, r.result.long("height")?.toInt() ?: 1, r.binary)
    }

    fun recentAgentActivity(limit: Int = 30): List<String> =
        engine.execute("history_list", mapOf("limit" to 200)).result.list("entries").orEmpty()
            .map { it as Map<*, *> }
            .filter { it["actor"] != "human" }
            .take(limit)
            .map { "r${it["revision"]}  ${it["actor"]}  ${it["op"]}  (${it["changedPixels"]} px)" }

    companion object {
        fun toImage(w: Int, h: Int, rgba: ByteArray): BufferedImage {
            val img = BufferedImage(w, h, BufferedImage.TYPE_INT_ARGB)
            if (rgba.size < w * h * 4) return img
            val argb = IntArray(w * h)
            for (p in 0 until w * h) {
                val o = p * 4
                argb[p] = ((rgba[o + 3].toInt() and 0xFF) shl 24) or ((rgba[o].toInt() and 0xFF) shl 16) or
                    ((rgba[o + 1].toInt() and 0xFF) shl 8) or (rgba[o + 2].toInt() and 0xFF)
            }
            img.setRGB(0, 0, w, h, argb, 0, w)
            return img
        }
    }
}

// ═══ CanvasView ══════════════════════════════════════════════════════════════════════════════════

/**
 * The drawing surface. Strokes are previewed locally and committed to the engine as ONE
 * command on mouse release (one undo step per stroke). Zoom: wheel; pan: middle/right drag.
 */
class CanvasView(private val c: EditorController) : JPanel() {
    private var zoom = 12.0
    private var offset = Point(40, 40)
    private var image = c.renderFrame(0)
    private val stroke = LinkedHashSet<Pair<Int, Int>>()
    private var anchor: Pair<Int, Int>? = null
    private var hover: Pair<Int, Int>? = null
    private var panStart: Point? = null
    private var centered = false

    var statusListener: (String) -> Unit = {}

    init {
        background = Theme.CANVAS_BG
        isFocusable = true
        cursor = Cursor.getPredefinedCursor(Cursor.CROSSHAIR_CURSOR)
        c.onChange { refresh() }
        val mouse = object : MouseAdapter() {
            override fun mousePressed(e: MouseEvent) = press(e)
            override fun mouseDragged(e: MouseEvent) = drag(e)
            override fun mouseReleased(e: MouseEvent) = release(e)
            override fun mouseMoved(e: MouseEvent) { hover = toPixel(e.point); status(); repaint() }
            override fun mouseWheelMoved(e: MouseWheelEvent) = wheel(e)
        }
        addMouseListener(mouse)
        addMouseMotionListener(mouse)
        addMouseWheelListener(mouse)
    }

    fun refresh() {
        image = c.renderFrame(c.doc.activeFrame, c.onionSkin)
        repaint()
    }

    fun zoomBy(factor: Double) {
        zoom = (zoom * factor).coerceIn(1.0, 96.0)
        repaint()
    }

    fun fit() {
        val d = c.doc
        zoom = max(1.0, min((width - 40.0) / d.width, (height - 40.0) / d.height)).let { kotlin.math.floor(it) }
        offset = Point(((width - d.width * zoom) / 2).toInt(), ((height - d.height * zoom) / 2).toInt())
        repaint()
    }

    private fun toPixel(p: Point): Pair<Int, Int> =
        Pair(kotlin.math.floor((p.x - offset.x) / zoom).toInt(), kotlin.math.floor((p.y - offset.y) / zoom).toInt())

    private fun inCanvas(px: Pair<Int, Int>) = px.first in 0 until c.doc.width && px.second in 0 until c.doc.height

    private fun status() {
        val h = hover
        val pos = if (h != null && inCanvas(h)) "x ${h.first}, y ${h.second}" else ""
        statusListener(pos)
    }

    private fun press(e: MouseEvent) {
        requestFocusInWindow()
        if (SwingUtilities.isMiddleMouseButton(e) || SwingUtilities.isRightMouseButton(e)) {
            panStart = e.point
            return
        }
        val px = toPixel(e.point)
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER -> { stroke.clear(); addStrokePoint(px) }
            Tool.LINE, Tool.RECTANGLE, Tool.ELLIPSE -> { anchor = px; hover = px }
            Tool.FILL -> if (inCanvas(px)) c.run("fill_region", mapOf("x" to px.first, "y" to px.second, "color" to c.color))
            Tool.PICKER -> if (inCanvas(px)) {
                c.run("get_pixel", mapOf("x" to px.first, "y" to px.second, "composite" to true)).result.str("color")?.let { c.color = it; c.reload() }
            }
        }
        repaint()
    }

    private var last: Pair<Int, Int>? = null

    private fun addStrokePoint(px: Pair<Int, Int>) {
        val l = last
        if (l == null || stroke.isEmpty()) {
            stroke += px
        } else {
            // Fill gaps between fast mouse samples with a Bresenham run.
            var (x0, y0) = l
            val (x1, y1) = px
            val dx = abs(x1 - x0); val dy = -abs(y1 - y0)
            val sx = if (x0 < x1) 1 else -1; val sy = if (y0 < y1) 1 else -1
            var err = dx + dy
            while (true) {
                stroke += Pair(x0, y0)
                if (x0 == x1 && y0 == y1) break
                val e2 = 2 * err
                if (e2 >= dy) { err += dy; x0 += sx }
                if (e2 <= dx) { err += dx; y0 += sy }
            }
        }
        last = px
    }

    private fun drag(e: MouseEvent) {
        panStart?.let {
            offset = Point(offset.x + e.x - it.x, offset.y + e.y - it.y)
            panStart = e.point
            repaint()
            return
        }
        val px = toPixel(e.point)
        hover = px
        if (c.tool == Tool.PENCIL || c.tool == Tool.ERASER) addStrokePoint(px)
        status()
        repaint()
    }

    private fun release(e: MouseEvent) {
        if (panStart != null) { panStart = null; return }
        val px = toPixel(e.point)
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER -> {
                val pixels = stroke.filter(::inCanvas).map { listOf(it.first, it.second) }
                if (pixels.isNotEmpty()) {
                    c.run("set_pixels", mapOf("pixels" to pixels, "color" to if (c.tool == Tool.ERASER) null else c.color))
                }
                stroke.clear()
                last = null
            }
            Tool.LINE -> anchor?.let { a -> c.run("draw_line", mapOf("x0" to a.first, "y0" to a.second, "x1" to px.first, "y1" to px.second, "color" to c.color)) }
            Tool.RECTANGLE, Tool.ELLIPSE -> anchor?.let { a ->
                val x = min(a.first, px.first); val y = min(a.second, px.second)
                val w = abs(px.first - a.first) + 1; val h = abs(px.second - a.second) + 1
                val op = if (c.tool == Tool.RECTANGLE) "draw_rectangle" else "draw_ellipse"
                c.run(op, mapOf("x" to x, "y" to y, "width" to w, "height" to h, "color" to c.color, "filled" to e.isShiftDown))
            }
            else -> {}
        }
        anchor = null
        repaint()
    }

    private fun wheel(e: MouseWheelEvent) {
        val before = toPixelD(e.point)
        zoom = (zoom * if (e.wheelRotation < 0) 1.2 else 1 / 1.2).coerceIn(1.0, 96.0)
        offset = Point((e.x - before.first * zoom).toInt(), (e.y - before.second * zoom).toInt())
        repaint()
    }

    private fun toPixelD(p: Point) = Pair((p.x - offset.x) / zoom, (p.y - offset.y) / zoom)

    override fun paintComponent(g0: Graphics) {
        super.paintComponent(g0)
        if (!centered && width > 0) { centered = true; fit() }
        val g = g0 as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
        val d = c.doc
        val w = (d.width * zoom).toInt(); val h = (d.height * zoom).toInt()
        // Transparency checkerboard.
        val cell = max(4, (zoom / 2).toInt())
        g.clipRect(offset.x, offset.y, w, h)
        var yy = 0
        while (yy < h) {
            var xx = 0
            while (xx < w) {
                g.color = if (((xx / cell) + (yy / cell)) % 2 == 0) Theme.CHECKER_A else Theme.CHECKER_B
                g.fillRect(offset.x + xx, offset.y + yy, cell, cell)
                xx += cell
            }
            yy += cell
        }
        g.drawImage(image, offset.x, offset.y, w, h, null)
        // Stroke / shape preview.
        g.color = if (c.tool == Tool.ERASER) Color(255, 255, 255, 120) else Theme.parse(c.color)
        for ((x, y) in stroke) g.fillRect(offset.x + (x * zoom).toInt(), offset.y + (y * zoom).toInt(), zoom.toInt().coerceAtLeast(1), zoom.toInt().coerceAtLeast(1))
        val a = anchor; val hv = hover
        if (a != null && hv != null) {
            g.stroke = BasicStroke(1f)
            val x = min(a.first, hv.first); val y = min(a.second, hv.second)
            val rw = abs(hv.first - a.first) + 1; val rh = abs(hv.second - a.second) + 1
            when (c.tool) {
                Tool.LINE -> g.drawLine(px(a.first, true), px(a.second, false), px(hv.first, true), px(hv.second, false))
                Tool.RECTANGLE -> g.drawRect(px(x, true) - (zoom / 2).toInt(), px(y, false) - (zoom / 2).toInt(), (rw * zoom).toInt(), (rh * zoom).toInt())
                Tool.ELLIPSE -> g.drawOval(px(x, true) - (zoom / 2).toInt(), px(y, false) - (zoom / 2).toInt(), (rw * zoom).toInt(), (rh * zoom).toInt())
                else -> {}
            }
        }
        if (c.showGrid && zoom >= 8) {
            g.color = Theme.GRID
            for (x in 0..d.width) g.drawLine(offset.x + (x * zoom).toInt(), offset.y, offset.x + (x * zoom).toInt(), offset.y + h)
            for (y in 0..d.height) g.drawLine(offset.x, offset.y + (y * zoom).toInt(), offset.x + w, offset.y + (y * zoom).toInt())
        }
        if (hv != null && inCanvas(hv)) {
            g.color = Theme.ACCENT
            g.drawRect(offset.x + (hv.first * zoom).toInt(), offset.y + (hv.second * zoom).toInt(), zoom.toInt(), zoom.toInt())
        }
        g.clip = null
        g.color = Theme.ACCENT_DIM
        g.drawRect(offset.x - 1, offset.y - 1, w + 1, h + 1)
    }

    private fun px(v: Int, isX: Boolean) = (if (isX) offset.x else offset.y) + (v * zoom + zoom / 2).toInt()
}

// ═══ SidePanels ══════════════════════════════════════════════════════════════════════════════════

/** Current color + palette swatches. Click = pick, double-click = edit entry (recolors art). */
class PalettePanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val grid = JPanel(GridLayout(0, 8, 2, 2))
    private val current = object : JPanel() {
        override fun paintComponent(g: Graphics) {
            super.paintComponent(g)
            g.color = Theme.parse(c.color)
            g.fillRect(2, 2, width - 4, height - 4)
        }
    }

    init {
        border = Theme.section("Palette")
        background = Theme.PANEL
        grid.background = Theme.PANEL
        current.preferredSize = Dimension(40, 28)
        current.toolTipText = "Current color — click to choose any color"
        current.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                JColorChooser.showDialog(this@PalettePanel, "Color", Theme.parse(c.color))?.let {
                    c.color = Theme.hex(it)
                    c.run("palette_set", mapOf("mode" to "add", "colors" to listOf(c.color)))
                    rebuild()
                }
            }
        })
        val top = JPanel(BorderLayout(6, 0)).apply {
            background = Theme.PANEL
            add(current, BorderLayout.WEST)
            add(JLabel("Click a swatch • double-click edits").apply { foreground = Theme.TEXT_DIM }, BorderLayout.CENTER)
        }
        add(top, BorderLayout.NORTH)
        add(grid, BorderLayout.CENTER)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun rebuild() {
        grid.removeAll()
        c.doc.palette.forEachIndexed { i, hex ->
            val sw = object : JPanel() {
                override fun paintComponent(g: Graphics) {
                    g.color = Theme.parse(hex)
                    g.fillRect(0, 0, width, height)
                    if (hex.equals(c.color, ignoreCase = true)) {
                        g.color = Color.WHITE
                        g.drawRect(0, 0, width - 1, height - 1)
                    }
                }
            }
            sw.preferredSize = Dimension(20, 20)
            sw.toolTipText = "#${i + 1} $hex"
            sw.addMouseListener(object : MouseAdapter() {
                override fun mouseClicked(e: MouseEvent) {
                    if (e.clickCount >= 2) {
                        JColorChooser.showDialog(this@PalettePanel, "Edit palette entry ${i + 1}", Theme.parse(hex))?.let {
                            c.run("palette_set", mapOf("index" to i + 1, "color" to Theme.hex(it)))
                        }
                    } else {
                        c.color = hex
                        rebuild()
                    }
                }
            })
            grid.add(sw)
        }
        current.repaint()
        revalidate()
        repaint()
    }
}

/** Layers, top of the stack first. */
class LayersPanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val model = DefaultListModel<LayerInfo>()
    private val list = JList(model)
    private var updating = false

    init {
        border = Theme.section("Layers")
        background = Theme.PANEL
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.cellRenderer = ListCellRenderer<LayerInfo> { _, value, _, selected, _ ->
            JLabel((if (value.visible) "◉ " else "○ ") + value.name + (if (value.locked) "  [locked]" else "") + (if (value.opacity < 255) "  ${value.opacity * 100 / 255}%" else "") + if (value.blend != "normal") "  · ${value.blend}" else "").apply {
                isOpaque = true
                border = BorderFactory.createEmptyBorder(4, 8, 4, 8)
                background = if (selected) Theme.PANEL_LIGHT else Theme.PANEL
                foreground = if (value.visible) Theme.TEXT else Theme.TEXT_DIM
            } as Component
        }
        list.addListSelectionListener {
            if (!updating && !it.valueIsAdjusting) list.selectedValue?.let { l -> c.run("layer_select", mapOf("layer" to l.index)) }
        }
        list.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                val idx = list.locationToIndex(e.point).takeIf { it >= 0 } ?: return
                val l = model[idx]
                if (e.x < 22) c.run("layer_set", mapOf("layer" to l.index, "visible" to !l.visible))
                else if (e.clickCount == 2) rename(l)
            }
        })
        add(JScrollPane(list).apply { preferredSize = Dimension(220, 160) }, BorderLayout.CENTER)
        val buttons = JPanel(FlowLayout(FlowLayout.LEFT, 2, 2)).apply {
            background = Theme.PANEL
            add(Theme.button("+", "New layer") { c.run("layer_create") })
            add(Theme.button("−", "Delete layer") {
                selected()?.let {
                    if (JOptionPane.showConfirmDialog(this, "Delete layer '${it.name}' in every frame?", "Delete layer", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                        c.run("layer_delete", mapOf("layer" to it.index))
                    }
                }
            })
            add(Theme.button("▲", "Move up") { selected()?.let { if (it.index < c.doc.layers.size - 1) c.run("layer_move", mapOf("layer" to it.index, "index" to it.index + 1)) } })
            add(Theme.button("▼", "Move down") { selected()?.let { if (it.index > 0) c.run("layer_move", mapOf("layer" to it.index, "index" to it.index - 1)) } })
            add(Theme.button("Lock", "Lock / unlock") { selected()?.let { c.run("layer_set", mapOf("layer" to it.index, "locked" to !it.locked)) } })
        }
        add(buttons, BorderLayout.SOUTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun selected(): LayerInfo? = list.selectedValue

    private fun rename(l: LayerInfo) {
        val name = JOptionPane.showInputDialog(this, "Layer name", l.name) ?: return
        if (name.isNotBlank()) c.run("layer_set", mapOf("layer" to l.index, "name" to name))
    }

    private fun rebuild() {
        updating = true
        model.clear()
        c.doc.layers.reversed().forEach { model.addElement(it) }
        val activeRow = c.doc.layers.size - 1 - c.doc.activeLayer
        if (activeRow in 0 until model.size()) list.selectedIndex = activeRow
        updating = false
    }
}

/** Live list of what connected agents have been doing. */
class ActivityPanel(private val c: EditorController) : JPanel() {
    private val model = DefaultListModel<String>()

    init {
        layout = BoxLayout(this, BoxLayout.Y_AXIS)
        border = Theme.section("Agent activity")
        background = Theme.PANEL
        val list = JList(model).apply { foreground = Theme.TEXT_DIM; font = font.deriveFont(11f) }
        add(JScrollPane(list).apply { preferredSize = Dimension(220, 140) })
        c.onChange { rebuild() }
        rebuild()
    }

    private fun rebuild() {
        model.clear()
        c.recentAgentActivity().forEach { model.addElement(it) }
        if (model.isEmpty) model.addElement("No agent edits yet")
    }
}

// ═══ TimelinePanel ═══════════════════════════════════════════════════════════════════════════════

/** Frame strip with thumbnails, per-frame durations, onion skin and playback. */
class TimelinePanel(private val c: EditorController, private val canvas: CanvasView) : JPanel(BorderLayout()) {
    private val model = DefaultListModel<FrameInfo>()
    private val list = JList(model)
    private val thumbs = HashMap<Int, BufferedImage>()
    private var thumbRevision = -1L
    private var updating = false
    private val duration = JSpinner(SpinnerNumberModel(100, 1, 60000, 10))
    private val info = JLabel()
    private var timer: Timer? = null
    private var playIndex = 0

    init {
        border = Theme.section("Timeline")
        background = Theme.PANEL
        list.layoutOrientation = JList.HORIZONTAL_WRAP
        list.visibleRowCount = 1
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.fixedCellWidth = 76
        list.fixedCellHeight = 84
        list.background = Theme.PANEL
        list.cellRenderer = ListCellRenderer<FrameInfo> { _, f, _, selected, _ -> Cell(f, selected) as Component }
        list.addListSelectionListener {
            if (!updating && !it.valueIsAdjusting) list.selectedValue?.let { f -> c.run("frame_select", mapOf("frame" to f.index)) }
        }
        add(JScrollPane(list).apply { preferredSize = Dimension(600, 108); border = BorderFactory.createEmptyBorder() }, BorderLayout.CENTER)

        val controls = JPanel(FlowLayout(FlowLayout.LEFT, 4, 2)).apply {
            background = Theme.PANEL
            add(Theme.button("▶", "Play / stop") { togglePlay() })
            add(Theme.button("+", "Add empty frame after the current one") { c.run("frame_add", mapOf("index" to c.doc.activeFrame + 1)) })
            add(Theme.button("⧉", "Duplicate frame") { c.run("frame_duplicate", mapOf("frame" to c.doc.activeFrame)) })
            add(Theme.button("−", "Delete frame") {
                if (c.doc.frames.size > 1 && JOptionPane.showConfirmDialog(this, "Delete frame ${c.doc.activeFrame}?", "Delete frame", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                    c.run("frame_delete", mapOf("frame" to c.doc.activeFrame))
                }
            })
            add(Theme.button("◀", "Move frame left") { if (c.doc.activeFrame > 0) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame - 1)) })
            add(Theme.button("▶|", "Move frame right") { if (c.doc.activeFrame < c.doc.frames.size - 1) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame + 1)) })
            add(JLabel("  ms").apply { foreground = Theme.TEXT_DIM })
            add(duration)
            add(Theme.button("Set", "Apply duration to the current frame") { c.run("frame_set_duration", mapOf("frame" to c.doc.activeFrame, "duration" to duration.value)) })
            add(JCheckBox("Onion skin").apply {
                isOpaque = false
                foreground = Theme.TEXT
                addActionListener { c.onionSkin = isSelected; canvas.refresh() }
            })
            add(info.apply { foreground = Theme.TEXT_DIM })
        }
        add(controls, BorderLayout.NORTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun thumb(index: Int): BufferedImage {
        if (thumbRevision != c.doc.revision) { thumbs.clear(); thumbRevision = c.doc.revision }
        return thumbs.getOrPut(index) { c.renderFrame(index) }
    }

    private inner class Cell(val f: FrameInfo, val selected: Boolean) : JPanel() {
        init { preferredSize = Dimension(76, 84); isOpaque = true }

        override fun paintComponent(g0: Graphics) {
            val g = g0 as Graphics2D
            g.color = if (selected) Theme.PANEL_LIGHT else Theme.PANEL
            g.fillRect(0, 0, width, height)
            val img = thumb(f.index)
            val box = 60
            val s = minOf(box.toDouble() / img.width, box.toDouble() / img.height)
            val w = (img.width * s).toInt(); val h = (img.height * s).toInt()
            val x = (width - w) / 2; val y = 4 + (box - h) / 2
            g.color = Theme.CHECKER_B
            g.fillRect(x, y, w, h)
            g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
            g.drawImage(img, x, y, w, h, null)
            g.color = if (selected) Theme.ACCENT else Color(255, 255, 255, 40)
            g.drawRect(x - 1, y - 1, w + 1, h + 1)
            g.color = Theme.TEXT_DIM
            g.drawString("${f.index}  ${f.duration}ms", 6, height - 6)
        }
    }

    private fun togglePlay() {
        timer?.let { it.stop(); timer = null; return }
        playIndex = c.doc.activeFrame
        val t = Timer(c.doc.frames.getOrNull(playIndex)?.duration ?: 100) {}
        t.addActionListener {
            val frames = c.doc.frames
            if (frames.isEmpty()) return@addActionListener
            playIndex = (playIndex + 1) % frames.size
            c.run("frame_select", mapOf("frame" to playIndex))
            t.delay = frames[playIndex].duration
        }
        t.isRepeats = true
        timer = t
        t.start()
    }

    private fun rebuild() {
        updating = true
        if (model.size() != c.doc.frames.size || (0 until model.size()).any { model[it] != c.doc.frames[it] }) {
            model.clear()
            c.doc.frames.forEach { model.addElement(it) }
        }
        list.selectedIndex = c.doc.activeFrame
        list.ensureIndexIsVisible(c.doc.activeFrame)
        list.repaint()
        c.doc.frames.getOrNull(c.doc.activeFrame)?.let { duration.value = it.duration }
        info.text = "  ${c.doc.frames.size} frames • ${c.doc.totalMs} ms • ${"%.1f".format(c.doc.fps)} fps"
        updating = false
    }
}

// ═══ VoxelPanel ══════════════════════════════════════════════════════════════════════════════════

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
        val script = javaClass.getResourceAsStream("/Knight.jsonl")?.bufferedReader()?.readLines() ?: return
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

// ═══ GameMenu ════════════════════════════════════════════════════════════════════════════════════

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
        item("Export atlas + Aseprite JSON…") { exportPair("atlas", "json") { r, base -> Json.write(r.result["data"]) } }
        item("Export Godot SpriteFrames…") { exportPair("godot", "tres") { r, _ -> r.result.str("tres") ?: "" } }
        item("Export Tiled map…") { exportTiled() }
    }

    private fun item(text: String, f: () -> Unit) = add(JMenuItem(text).apply { addActionListener { f() } })

    private fun form(title: String, vararg rows: Pair<String, java.awt.Component>): Boolean {
        val p = JPanel(GridLayout(rows.size, 2, 6, 4))
        rows.forEach { (label, comp) -> p.add(JLabel(label)); p.add(comp) }
        return JOptionPane.showConfirmDialog(owner, p, title, JOptionPane.OK_CANCEL_OPTION) == JOptionPane.OK_OPTION
    }

    private fun spinner(v: Int, min: Int, max: Int) = JSpinner(SpinnerNumberModel(v, min, max, 1))

    private fun report(r: com.pixel.studio.gateway.EngineResponse) {
        if (!r.ok) JOptionPane.showMessageDialog(owner, r.errorMessage ?: "failed", "Pixel Studio", JOptionPane.WARNING_MESSAGE)
    }

    private fun blendMode() {
        val modes = arrayOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light")
        val current = c.doc.layers.getOrNull(c.doc.activeLayer)?.blend ?: "normal"
        val box = JComboBox(modes).apply { selectedItem = current }
        if (form("Blend mode of the active layer", "Mode" to box)) report(c.run("layer_set", mapOf("blend" to box.selectedItem)))
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
        if (form("RotSprite (clean pixel-art rotation)", "Degrees (clockwise)" to deg)) report(c.run("rotate", mapOf("method" to "rotsprite", "degrees" to deg.value)))
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

    private fun exportPair(format: String, ext: String, doc: (com.pixel.studio.gateway.EngineResponse, String) -> String) {
        val png = chooser(c.doc.name) ?: return
        val base = png.name.removeSuffix(".png")
        val r = c.run("export", mapOf("format" to format, "image" to png.name, "texture_path" to "res://${png.name}"))
        if (!r.ok) return report(r)
        png.writeBytes(r.binary)
        File(png.parentFile, "$base.$ext").writeText(doc(r, base))
    }

    private fun exportTiled() {
        val png = chooser(c.doc.name + "_tileset") ?: return
        val r = c.run("export", mapOf("format" to "tiled", "image" to png.name))
        if (!r.ok) return report(r)
        png.writeBytes(r.binary)
        File(png.parentFile, png.name.removeSuffix("_tileset.png").removeSuffix(".png") + ".tmj").writeText(Json.write(r.result["tmj"]))
    }
}

// ═══ AgentDialog ═════════════════════════════════════════════════════════════════════════════════

/** "Connect an agent": endpoint, token and copy-paste snippets for Claude Code, Antigravity, etc. */
class AgentDialog(owner: JFrame, private val gateway: AgentGateway, private val onToggle: () -> Unit) : JDialog(owner, "Connect an AI agent", false) {
    private val body = JPanel()

    init {
        body.layout = BoxLayout(body, BoxLayout.Y_AXIS)
        body.background = Theme.PANEL
        body.border = BorderFactory.createEmptyBorder(12, 12, 12, 12)
        contentPane.add(JScrollPane(body), BorderLayout.CENTER)
        rebuild()
        size = Dimension(760, 560)
        setLocationRelativeTo(owner)
    }

    private fun rebuild() {
        body.removeAll()
        val state = if (gateway.running) "Agent Mode is ON — ${gateway.mcpUrl}" else "Agent Mode is OFF"
        body.add(JLabel(state).apply { foreground = if (gateway.running) Theme.ACCENT else Theme.TEXT_DIM; font = font.deriveFont(Font.BOLD, 15f) })
        body.add(JLabel("Agents can only use Pixel Studio's drawing tools. Deleting frames/layers asks you first.").apply { foreground = Theme.TEXT_DIM })
        body.add(JPanel(FlowLayout(FlowLayout.LEFT)).apply {
            isOpaque = false
            add(Theme.button(if (gateway.running) "Stop Agent Mode" else "Start Agent Mode") { onToggle(); rebuild() })
            add(Theme.button("New token", "Disconnects every agent using the old token") { gateway.access.rotateToken(); rebuild() })
        })
        if (gateway.running) {
            for ((title, snippet) in gateway.connectionSnippets()) {
                body.add(JLabel(title).apply { foreground = Theme.ACCENT_2; border = BorderFactory.createEmptyBorder(10, 0, 4, 0) })
                val area = JTextArea(snippet).apply {
                    isEditable = false
                    lineWrap = true
                    font = Font(Font.MONOSPACED, Font.PLAIN, 12)
                    background = Theme.BG
                    foreground = Theme.TEXT
                    border = BorderFactory.createEmptyBorder(6, 6, 6, 6)
                }
                body.add(area)
                body.add(JPanel(FlowLayout(FlowLayout.LEFT, 0, 2)).apply {
                    isOpaque = false
                    add(Theme.button("Copy") { Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(snippet), null) })
                })
            }
            body.add(JLabel("Then ask your agent e.g.: \"Use Pixel Studio. Draw a 32x32 knight, then make a 6-frame idle animation.\"").apply {
                foreground = Theme.TEXT_DIM
                border = BorderFactory.createEmptyBorder(12, 0, 0, 0)
            })
        }
        body.revalidate()
        body.repaint()
    }
}

// ═══ Theme ═══════════════════════════════════════════════════════════════════════════════════════

/** Dark neon theme matching the Pixel Studio icon (cyan → violet on deep navy). */
object Theme {
    val BG = Color(0x0B, 0x10, 0x24)
    val PANEL = Color(0x12, 0x18, 0x33)
    val PANEL_LIGHT = Color(0x1B, 0x23, 0x47)
    val CANVAS_BG = Color(0x08, 0x0C, 0x1C)
    val TEXT = Color(0xE6, 0xEA, 0xFF)
    val TEXT_DIM = Color(0x8C, 0x95, 0xC4)
    val ACCENT = Color(0x38, 0xE1, 0xFF)
    val ACCENT_2 = Color(0xA8, 0x55, 0xF7)
    val ACCENT_DIM = Color(0x38, 0xE1, 0xFF, 90)
    val GRID = Color(255, 255, 255, 28)
    val CHECKER_A = Color(0x2A, 0x2F, 0x45)
    val CHECKER_B = Color(0x22, 0x27, 0x3B)

    fun install() {
        runCatching { UIManager.setLookAndFeel("javax.swing.plaf.nimbus.NimbusLookAndFeel") }
        UIManager.put("control", PANEL)
        UIManager.put("info", PANEL_LIGHT)
        UIManager.put("nimbusBase", Color(0x1A, 0x1F, 0x3D))
        UIManager.put("nimbusBlueGrey", Color(0x2A, 0x31, 0x5C))
        UIManager.put("nimbusLightBackground", PANEL_LIGHT)
        UIManager.put("nimbusFocus", ACCENT)
        UIManager.put("nimbusSelectionBackground", Color(0x3B, 0x2A, 0x7A))
        UIManager.put("nimbusSelectedText", TEXT)
        UIManager.put("text", TEXT)
        UIManager.put("defaultFont", Font(Font.SANS_SERIF, Font.PLAIN, 13))
    }

    fun section(title: String): Border = BorderFactory.createCompoundBorder(
        BorderFactory.createEmptyBorder(6, 6, 6, 6),
        BorderFactory.createTitledBorder(BorderFactory.createLineBorder(PANEL_LIGHT), title, 0, 0, null, ACCENT),
    )

    fun button(text: String, tip: String? = null, action: () -> Unit) = JButton(text).apply {
        isFocusable = false
        toolTipText = tip
        addActionListener { action() }
    }

    fun parse(hex: String): Color {
        val h = hex.removePrefix("#")
        return when (h.length) {
            6 -> Color(h.toInt(16))
            8 -> Color(h.substring(0, 2).toInt(16), h.substring(2, 4).toInt(16), h.substring(4, 6).toInt(16), h.substring(6, 8).toInt(16))
            else -> Color.MAGENTA
        }
    }

    fun hex(c: Color): String = if (c.alpha == 255) String.format("#%02X%02X%02X", c.red, c.green, c.blue)
    else String.format("#%02X%02X%02X%02X", c.red, c.green, c.blue, c.alpha)
}
