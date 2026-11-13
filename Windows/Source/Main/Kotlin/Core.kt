// Pixel Studio — desktop editor (Windows 10/11, Linux, macOS; JDK Swing): 2D pixel and 3D voxel workspaces.

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
import java.awt.AlphaComposite
import java.awt.BasicStroke
import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Cursor
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Font
import java.awt.GradientPaint
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.GraphicsEnvironment
import java.awt.GridLayout
import java.awt.Image
import java.awt.Insets
import java.awt.Point
import java.awt.Rectangle
import java.awt.RenderingHints
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import java.awt.event.KeyEvent
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.awt.geom.Path2D
import java.awt.geom.RoundRectangle2D
import java.awt.image.BufferedImage
import java.io.File
import java.net.BindException
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.imageio.ImageIO
import javax.swing.AbstractButton
import javax.swing.BorderFactory
import javax.swing.Box
import javax.swing.BoxLayout
import javax.swing.ButtonGroup
import javax.swing.DefaultListModel
import javax.swing.Icon
import javax.swing.ImageIcon
import javax.swing.JButton
import javax.swing.JCheckBox
import javax.swing.JColorChooser
import javax.swing.JComboBox
import javax.swing.JComponent
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
import javax.swing.JScrollBar
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
import javax.swing.SwingConstants
import javax.swing.SwingUtilities
import javax.swing.Timer
import javax.swing.UIManager
import javax.swing.border.Border
import javax.swing.filechooser.FileNameExtensionFilter
import javax.swing.plaf.basic.BasicButtonUI
import javax.swing.plaf.basic.BasicScrollBarUI
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

// ═══ PixelStudioApp ══════════════════════════════════════════════════════════════════════════════

const val APP_ID = "com.pixel.studio"

class PixelStudioWindow(private val c: EditorController, private val gateway: AgentGateway) : JFrame("Pixel Studio") {
    private val canvas = CanvasView(c)
    val voxels = VoxelPanel(c)
    private val status = StatusBar(c, gateway)
    private val toolButtons = HashMap<Tool, AbstractButton>()

    init {
        defaultCloseOperation = DO_NOTHING_ON_CLOSE
        iconImages = listOf("icon-32.png", "icon-64.png", "icon-256.png").mapNotNull { name ->
            javaClass.getResource("/$name")?.let { ImageIO.read(it) as Image }
        }
        jMenuBar = menus()
        contentPane.background = Theme.BG
        contentPane.layout = BorderLayout()

        val pixel2d = JPanel(BorderLayout()).apply { background = Theme.BG }
        pixel2d.add(OptionsBar(c, canvas), BorderLayout.NORTH)
        pixel2d.add(toolRail(), BorderLayout.WEST)
        pixel2d.add(canvas, BorderLayout.CENTER)
        pixel2d.add(Dock(c), BorderLayout.EAST)
        pixel2d.add(TimelinePanel(c, canvas), BorderLayout.SOUTH)

        val tabs = Theme.tabs()
        tabs.addTab("Pixel", pixel2d)
        tabs.addTab("Voxel 3D", voxels)
        Theme.styleTree(voxels)
        contentPane.add(tabs, BorderLayout.CENTER)
        contentPane.add(status, BorderLayout.SOUTH)

        canvas.statusListener = { status.cursor = it; status.refresh() }
        c.onChange { status.refresh(); title = "Pixel Studio — ${c.doc.name}" + (c.file?.let { " (${it.name})" } ?: "") }
        c.onToolChange { toolButtons[c.tool]?.isSelected = true }
        installShortcuts()
        addWindowListener(object : WindowAdapter() {
            override fun windowClosing(e: WindowEvent) = quit()
        })
        size = Dimension(1440, 900)
        minimumSize = Dimension(1024, 680)
        setLocationRelativeTo(null)
    }

    private fun toolRail(): JPanel {
        val rail = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS)
            background = Theme.PANEL
            border = BorderFactory.createCompoundBorder(
                BorderFactory.createMatteBorder(0, 0, 0, 1, Theme.BORDER),
                BorderFactory.createEmptyBorder(8, 6, 8, 6),
            )
        }
        val group = ButtonGroup()
        var lastGroup = Tool.entries.first().group
        for (t in Tool.entries) {
            if (t.group != lastGroup) { rail.add(Box.createVerticalStrut(6)); rail.add(Theme.separator()); rail.add(Box.createVerticalStrut(6)); lastGroup = t.group }
            val b = Theme.iconToggle(ToolIcon(t), "${t.label}  (${t.key})\n${t.hint}") { c.tool = t }
            b.isSelected = t == c.tool
            group.add(b)
            toolButtons[t] = b
            rail.add(b)
            rail.add(Box.createVerticalStrut(2))
        }
        rail.add(Box.createVerticalStrut(10))
        rail.add(ColorChips(c))
        rail.add(Box.createVerticalGlue())
        return rail
    }

    private fun installShortcuts() {
        val im = rootPane.getInputMap(JComponent.WHEN_IN_FOCUSED_WINDOW)
        val am = rootPane.actionMap
        fun bind(key: KeyStroke, name: String, f: () -> Unit) { im.put(key, name); am.put(name, action(f)) }
        for (t in Tool.entries) bind(KeyStroke.getKeyStroke(t.key), "tool-$t") { c.tool = t }
        bind(KeyStroke.getKeyStroke('x'), "swap") { c.swapColors() }
        bind(KeyStroke.getKeyStroke('['), "size-") { c.brushSize = (c.brushSize - 1).coerceAtLeast(1); c.fireTool() }
        bind(KeyStroke.getKeyStroke(']'), "size+") { c.brushSize = (c.brushSize + 1).coerceAtMost(4); c.fireTool() }
        bind(KeyStroke.getKeyStroke(KeyEvent.VK_DELETE, 0), "delete") { canvas.deleteSelection() }
        bind(KeyStroke.getKeyStroke(KeyEvent.VK_ESCAPE, 0), "deselect") { canvas.clearSelection() }
        bind(KeyStroke.getKeyStroke('0'), "fit") { canvas.fit() }
        bind(KeyStroke.getKeyStroke('+'), "zoomin") { canvas.zoomBy(1.25) }
        bind(KeyStroke.getKeyStroke('-'), "zoomout") { canvas.zoomBy(0.8) }
    }

    private fun action(f: () -> Unit) = object : javax.swing.AbstractAction() {
        override fun actionPerformed(e: java.awt.event.ActionEvent?) = f()
    }

    private fun item(text: String, key: KeyStroke? = null, f: () -> Unit) = JMenuItem(text).apply {
        if (key != null) accelerator = key
        addActionListener { f() }
    }

    private fun ctrl(k: Int, shift: Boolean = false) =
        KeyStroke.getKeyStroke(k, Toolkit.getDefaultToolkit().menuShortcutKeyMaskEx or if (shift) KeyEvent.SHIFT_DOWN_MASK else 0)

    private fun menus() = JMenuBar().apply {
        add(JMenu("File").apply {
            add(item("New…", ctrl(KeyEvent.VK_N)) { newProject() })
            add(item("Open…", ctrl(KeyEvent.VK_O)) { open() })
            add(item("Import image…", ctrl(KeyEvent.VK_I, true)) { importImage(false) })
            add(item("Import reference…") { importImage(true) })
            addSeparator()
            add(item("Save", ctrl(KeyEvent.VK_S)) { save(false) })
            add(item("Save As…", ctrl(KeyEvent.VK_S, true)) { save(true) })
            addSeparator()
            add(item("Export PNG…", ctrl(KeyEvent.VK_E)) { export("png") })
            add(item("Export GIF…") { export("gif") })
            add(item("Export sprite sheet…") { export("spritesheet") })
            add(item("Export normal map…") { export("normal_map") })
            addSeparator()
            add(item("Exit", ctrl(KeyEvent.VK_Q)) { quit() })
        })
        add(JMenu("Edit").apply {
            add(item("Undo", ctrl(KeyEvent.VK_Z)) { c.run("history_undo") })
            add(item("Redo", ctrl(KeyEvent.VK_Y)) { c.run("history_redo") })
            addSeparator()
            add(item("Select all", ctrl(KeyEvent.VK_A)) { canvas.selectAll() })
            add(item("Deselect", ctrl(KeyEvent.VK_D)) { canvas.clearSelection() })
            add(item("Delete selection") { canvas.deleteSelection() })
            addSeparator()
            add(item("Flip horizontal") { c.run("flip", canvas.regionArgs() + ("axis" to "horizontal")) })
            add(item("Flip vertical") { c.run("flip", canvas.regionArgs() + ("axis" to "vertical")) })
            add(item("Rotate…") { rotate() })
        })
        add(JMenu("Image").apply {
            add(item("Canvas size…") { resize() })
            addSeparator()
            add(item("Outline: one dark color") { c.run("outline", mapOf("color" to c.color, "mode" to "solid")) })
            add(item("Outline: selective") { c.run("outline", mapOf("mode" to "selective")) })
            add(item("Outline: selout (soft)") { c.run("outline", mapOf("mode" to "selout")) })
            add(item("Review art…", ctrl(KeyEvent.VK_R, true)) { reviewArt() })
            addSeparator()
            add(item("Form light layer (ramps)") { report(c.run("shade_form", mapOf("light" to "top-left", "shadow_steps" to 2, "light_steps" to 2))) })
            add(item("Auto shade layer (ramps)") { c.run("auto_shade", mapOf("light" to "top-left", "tones" to 3)) })
            add(item("Fix banding (layer)") { report(c.run("fix_banding")) })
            add(item("Selective anti-alias (layer)") { report(c.run("antialias", mapOf("amount" to 0.5))) })
            add(item("Pixel-perfect cleanup") { c.run("pixel_cleanup") })
            addSeparator()
            add(item("Reduce colors…") {
                val n = JOptionPane.showInputDialog(this@PixelStudioWindow, "Number of colors", "32")?.toIntOrNull() ?: return@item
                c.run("quantize", mapOf("scope" to "frame", "colors" to n))
            })
            val names = c.engine.execute("canvas_inspect", mapOf("detail" to "palette")).result.list("presets").orEmpty()
                .mapNotNull { (it as? Map<*, *>)?.get("name") as? String }
            add(JMenu("Load palette").apply { names.forEach { n -> add(item(n) { c.run("palette_set", mapOf("preset" to n)) }) } })
        })
        add(GameMenu(this@PixelStudioWindow, c) { workspaceDir() })
        add(JMenu("View").apply {
            add(item("Zoom in", ctrl(KeyEvent.VK_EQUALS)) { canvas.zoomBy(1.25) })
            add(item("Zoom out", ctrl(KeyEvent.VK_MINUS)) { canvas.zoomBy(0.8) })
            add(item("Fit to window", ctrl(KeyEvent.VK_0)) { canvas.fit() })
            add(item("Actual pixels ×8") { canvas.setZoom(8.0) })
            addSeparator()
            add(item("Toggle pixel grid", ctrl(KeyEvent.VK_QUOTE)) { c.showGrid = !c.showGrid; c.fireTool() })
            add(item("Toggle onion skin") { c.onionSkin = !c.onionSkin; c.fireTool(); canvas.refresh() })
        })
        add(JMenu("Agent").apply {
            add(item("Connect an AI agent…", ctrl(KeyEvent.VK_K)) { AgentDialog(this@PixelStudioWindow, gateway) { toggleGateway() }.isVisible = true })
        })
    }

    private fun report(r: EngineResponse) {
        if (r.ok) status.flash(r.result.entries.joinToString("  ·  ") { "${it.key} ${it.value}" }.take(160))
    }

    /** The critic: score, issues with their fixes, and the marked value map. */
    private fun reviewArt() {
        val r = c.run("art_review", mapOf("frame" to c.doc.activeFrame))
        if (!r.ok) return
        val res = r.result
        val img = ImageIO.read(r.binary.inputStream())
        val d = JDialog(this, "Art review", false)
        val text = StringBuilder()
        val v = res.obj("values")
        text.append("Score ${res["score"]} / 100\n")
        text.append("Values: dark ${v?.get("p10")} · median ${v?.get("median")} · light ${v?.get("p90")} · span ${v?.get("span")} (aim ≥ 55)\n")
        text.append("Banding pairs ${res.obj("banding")?.get("pairs")} · stripes ${res["stripesPercent"]}% · jaggies ${res["jaggies"]} · colours ${res["colors"]}\n\n")
        val issues = res.list("issues").orEmpty().map { it as Map<*, *> }
        if (issues.isEmpty()) text.append("No issues found.\n")
        issues.forEach { text.append("• [${it["severity"]}] ${it["message"]}\n    → ${it["fix"]}\n") }
        text.append("\n").append(res.str("legend") ?: "")
        val area = JTextArea(text.toString()).apply {
            isEditable = false; lineWrap = true; wrapStyleWord = true; background = Theme.PANEL; foreground = Theme.TEXT
            border = BorderFactory.createEmptyBorder(12, 12, 12, 12)
        }
        val pic = JLabel(ImageIcon(img)).apply { border = BorderFactory.createEmptyBorder(8, 8, 8, 8) }
        d.contentPane.background = Theme.PANEL
        d.contentPane.add(JScrollPane(pic).apply { Theme.scroll(this); border = BorderFactory.createEmptyBorder() }, BorderLayout.CENTER)
        d.contentPane.add(JScrollPane(area).apply { preferredSize = Dimension(380, 400); Theme.scroll(this); border = BorderFactory.createMatteBorder(0, 1, 0, 0, Theme.BORDER) }, BorderLayout.EAST)
        d.size = Dimension(min(1200, img.width + 440), min(860, img.height + 80))
        d.setLocationRelativeTo(this)
        d.isVisible = true
    }

    private fun toggleGateway() {
        if (gateway.running) gateway.stop() else runCatching { gateway.start() }.onFailure {
            JOptionPane.showMessageDialog(this, "Could not start Agent Mode: ${it.message}")
        }
        status.refresh()
    }

    private fun sizeDialog(title: String, w: Int, h: Int): Pair<Int, Int>? {
        val ws = JSpinner(SpinnerNumberModel(w, 1, 4096, 1))
        val hs = JSpinner(SpinnerNumberModel(h, 1, 4096, 1))
        val presets = JComboBox(arrayOf("Custom", "16 × 16", "32 × 32", "64 × 64", "128 × 128", "256 × 256", "320 × 180", "480 × 270")).apply {
            addActionListener {
                val v = (selectedItem as String).split(" × ").mapNotNull { it.toIntOrNull() }
                if (v.size == 2) { ws.value = v[0]; hs.value = v[1] }
            }
        }
        val p = JPanel(GridLayout(0, 2, 8, 6)).apply {
            add(JLabel("Preset")); add(presets); add(JLabel("Width (px)")); add(ws); add(JLabel("Height (px)")); add(hs)
        }
        return if (JOptionPane.showConfirmDialog(this, p, title, JOptionPane.OK_CANCEL_OPTION, JOptionPane.PLAIN_MESSAGE) == JOptionPane.OK_OPTION) {
            Pair(ws.value as Int, hs.value as Int)
        } else null
    }

    private fun newProject() {
        val (w, h) = sizeDialog("New sprite", 64, 64) ?: return
        c.run("project_new", mapOf("width" to w, "height" to h, "name" to "Untitled"))
        c.file = null
        canvas.fit()
    }

    private fun resize() {
        val (w, h) = sizeDialog("Canvas size", c.doc.width, c.doc.height) ?: return
        c.run("canvas_resize", mapOf("width" to w, "height" to h))
        canvas.fit()
    }

    private fun rotate() {
        val deg = JOptionPane.showInputDialog(this, "Clockwise degrees (90/180/270 exact, any other angle uses RotSprite)", "90")?.toDoubleOrNull() ?: return
        c.run("rotate", canvas.regionArgs() + ("degrees" to deg))
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

    private fun importImage(reference: Boolean) {
        val ch = chooser("png", "PNG image (*.png)")
        if (ch.showOpenDialog(this) != JFileChooser.APPROVE_OPTION) return
        val args = linkedMapOf<String, Any?>("grid" to "auto", "ramps" to true)
        if (reference) args["reference"] = true else args["layer"] = ch.selectedFile.nameWithoutExtension
        val r = c.run("image_import", args, ch.selectedFile.readBytes())
        if (r.ok) {
            val n = r.result.obj("native")
            status.flash("Imported ${n?.long("width")}×${n?.long("height")} (scale ${r.result["scale"]}), ${r.result["colors"]} colors, ${r.result.list("ramps")?.size ?: 0} ramps")
        }
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
            status.flash("Saved ${f.name}")
        }
    }

    private fun export(format: String) {
        val ext = if (format == "gif") "gif" else "png"
        val scale = JOptionPane.showInputDialog(this, "Scale (1-64)", "4")?.toIntOrNull()?.coerceIn(1, 64) ?: return
        val ch = chooser(ext, ext.uppercase())
        val suffix = when (format) { "spritesheet" -> "_sheet"; "normal_map" -> "_normal"; else -> "" }
        ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + suffix + "." + ext)
        if (ch.showSaveDialog(this) != JFileChooser.APPROVE_OPTION) return
        val r = c.run("export", mapOf("format" to format, "scale" to scale, "frame" to c.doc.activeFrame))
        if (!r.ok) return
        ch.selectedFile.writeBytes(r.binary)
        if (format == "spritesheet") File(ch.selectedFile.path.removeSuffix(".png") + ".json").writeText(Json.write(r.result))
        status.flash("Exported ${ch.selectedFile.name}")
    }

    private fun quit() {
        gateway.close()
        dispose()
        c.engine.close()
        System.exit(0)
    }

    /** Paints the whole window into an image (used for docs/screenshots and UI smoke tests). */
    fun snapshot(): BufferedImage {
        val root = rootPane
        val img = BufferedImage(root.width, root.height, BufferedImage.TYPE_INT_ARGB)
        val g = img.createGraphics()
        root.paint(g)
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
    System.setProperty("swing.aatext", "true")
    val engine = PixelEngine(64, 64)
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
        val toolIndex = args.indexOf("--tool")
        if (scriptIndex >= 0 && scriptIndex + 1 < args.size) {
            File(args[scriptIndex + 1]).readLines().filter { it.isNotBlank() }.forEach { engine.executeRaw(it.toByteArray()) }
        }
        args.firstOrNull { it.endsWith(".pxs") }?.let { w.openFile(File(it)) }
        if (toolIndex >= 0) Tool.entries.firstOrNull { it.name.equals(args.getOrNull(toolIndex + 1), true) }?.let { c.tool = it }
        w.isVisible = true
        if (tabIndex >= 0 && args.getOrNull(tabIndex + 1) == "3d") {
            (w.contentPane.components.firstOrNull { it is javax.swing.JTabbedPane } as? javax.swing.JTabbedPane)?.selectedIndex = 1
            if ("--hero" in args) w.voxels.javaClass.getDeclaredMethod("loadHero").apply { isAccessible = true }.invoke(w.voxels)
        }
        if (screenshotIndex >= 0 && screenshotIndex + 1 < args.size) {
            javax.swing.Timer(if ("--hero" in args) 12000 else 2000) {
                ImageIO.write(w.snapshot(), "png", File(args[screenshotIndex + 1]))
                finalGateway.close()
                System.exit(0)
            }.apply { isRepeats = false }.start()
        }
    }
}

// ═══ EditorController ════════════════════════════════════════════════════════════════════════════

enum class Tool(val label: String, val key: Char, val group: Int, val hint: String) {
    PENCIL("Pencil", 'b', 0, "Pixel-perfect freehand. Shift: straight line from the last point."),
    ERASER("Eraser", 'e', 0, "Erases to transparent."),
    SHADE("Shade", 'd', 0, "Steps pixels along their material ramp. Left = darker, right = lighter."),
    FILL("Fill", 'g', 0, "Bucket fill (contiguous or all pixels of that color)."),
    PICKER("Eyedropper", 'i', 0, "Picks a color. Right click = secondary color."),
    LINE("Line", 'l', 1, "Straight line."),
    RECTANGLE("Rectangle", 'r', 1, "Rectangle. Shift or the Filled option fills it."),
    ELLIPSE("Ellipse", 'o', 1, "Ellipse. Shift or the Filled option fills it."),
    SELECT("Select", 'm', 2, "Rectangular selection. Delete clears it, flip/rotate act on it."),
    MOVE("Move", 'v', 2, "Moves the selection (or the whole layer)."),
    HAND("Hand", 'h', 2, "Pans the view. Space, middle or right drag works with every tool."),
}

data class LayerInfo(
    val index: Int, val name: String, val visible: Boolean, val locked: Boolean, val opacity: Int,
    val blend: String = "normal", val density: Int = 1,
)
data class FrameInfo(val index: Int, val duration: Int)
data class RampInfo(val name: String, val colors: List<String>)

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
    val ramps: List<RampInfo> = emptyList(),
    val canUndo: Boolean = false,
    val canRedo: Boolean = false,
    val totalMs: Long = 0,
    val fps: Double = 0.0,
)

/**
 * UI-side controller. Every edit goes to the shared engine with actor "human"; agents edit
 * the same engine through the gateway, and both kinds of change arrive here as revision events.
 */
class EditorController(val engine: PixelEngine) {
    var tool = Tool.PENCIL
        set(v) { field = v; fireTool() }
    var color = "#1B1F2E"
    var color2 = "#F2EFE6"
    var brushSize = 1
    var pixelPerfect = true
    var mirrorX = false
    var mirrorY = false
    var filledShapes = false
    var contiguous = true
    var shadeStep = 1
    var onionSkin = false
    var showGrid = true
    var file: File? = null
    @Volatile var doc = DocInfo()
        private set

    private val listeners = mutableListOf<() -> Unit>()
    private val toolListeners = mutableListOf<() -> Unit>()
    private val refreshQueued = AtomicBoolean(false)

    /** Shown in the status bar; set when an engine call fails. */
    var lastError: String? = null
        private set

    init {
        engine.addChangeListener { scheduleRefresh() }
        reload()
    }

    fun onChange(l: () -> Unit) { listeners += l }
    fun onToolChange(l: () -> Unit) { toolListeners += l }
    fun fireTool() = toolListeners.forEach { it() }

    fun swapColors() { val t = color; color = color2; color2 = t; fireTool() }
    fun pick(hex: String, secondary: Boolean = false) { if (secondary) color2 = hex else color = hex; fireTool() }

    val activeDensity: Int get() = doc.layers.getOrNull(doc.activeLayer)?.density ?: 1
    val activeLocked: Boolean get() = doc.layers.getOrNull(doc.activeLayer)?.locked ?: false

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
        val p = engine.execute("canvas_inspect", mapOf("detail" to "palette")).result
        doc = DocInfo(
            name = i.str("name") ?: "Untitled",
            width = i.long("width")!!.toInt(),
            height = i.long("height")!!.toInt(),
            revision = i.long("revision") ?: 0,
            layers = i.list("layers")!!.map {
                @Suppress("UNCHECKED_CAST") val l = it as Map<String, Any?>
                LayerInfo(l.long("index")!!.toInt(), l.str("name")!!, l.bool("visible")!!, l.bool("locked")!!, l.long("opacity")!!.toInt(),
                    l.str("blend") ?: "normal", l.long("density")?.toInt() ?: 1)
            },
            frames = i.list("frames")!!.map {
                @Suppress("UNCHECKED_CAST") val f = it as Map<String, Any?>
                FrameInfo(f.long("index")!!.toInt(), f.long("duration")!!.toInt())
            },
            activeLayer = i.long("activeLayer")!!.toInt(),
            activeFrame = i.long("activeFrame")!!.toInt(),
            palette = p.list("colors")?.mapNotNull { (it as? Map<*, *>)?.get("color") as? String } ?: emptyList(),
            ramps = p.list("ramps")?.mapNotNull { r ->
                (r as? Map<*, *>)?.let { RampInfo(it["name"] as String, (it["colors"] as List<*>).map { c -> c as String }) }
            } ?: emptyList(),
            canUndo = i["undoAvailable"] == true,
            canRedo = i["redoAvailable"] == true,
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

    fun renderFrame(frame: Int, onion: Boolean = false, layer: Int? = null, density: Int? = null): BufferedImage {
        val args = linkedMapOf<String, Any?>("frame" to frame)
        if (onion) args["onion"] = mapOf("before" to 1, "after" to 1, "opacity" to 90)
        if (layer != null) args["layer"] = layer
        if (density != null) args["density"] = density
        val r = engine.execute("render_frame", args)
        return toImage(r.result.long("width")?.toInt() ?: 1, r.result.long("height")?.toInt() ?: 1, r.binary)
    }

    fun recentAgentActivity(limit: Int = 40): List<String> =
        engine.execute("history_list", mapOf("limit" to 200)).result.list("entries").orEmpty()
            .map { it as Map<*, *> }
            .filter { it["actor"] != "human" }
            .take(limit)
            .map { "r${it["revision"]}  ${it["actor"]}  ${it["op"]}  ${it["changedPixels"]} px" }

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
 * command on mouse release (one undo step per stroke). Coordinates follow the active layer's
 * pixel density, so a 2× or 4× detail layer is painted with half/quarter-size pixels.
 * Zoom: wheel (at the cursor); pan: Hand tool, Space, middle or right drag.
 */
class CanvasView(private val c: EditorController) : JPanel() {
    private var zoom = 8.0
    private var offset = Point(40, 40)
    private var image = c.renderFrame(0)
    private val stroke = LinkedHashSet<Pair<Int, Int>>()
    private var strokeSecondary = false
    private var anchor: Pair<Int, Int>? = null
    private var hover: Pair<Int, Int>? = null
    private var panStart: Point? = null
    private var spaceDown = false
    private var centered = false
    private var last: Pair<Int, Int>? = null
    /** Selection in canvas pixels: x, y, width, height. */
    private var selection: Rectangle? = null
    private var moveStart: Pair<Int, Int>? = null
    private var moveDelta = Pair(0, 0)
    private var ants = 0f

    var statusListener: (String) -> Unit = {}

    init {
        background = Theme.CANVAS_BG
        isFocusable = true
        c.onChange { refresh() }
        c.onToolChange { updateCursor(); repaint() }
        val mouse = object : MouseAdapter() {
            override fun mousePressed(e: MouseEvent) = press(e)
            override fun mouseDragged(e: MouseEvent) = drag(e)
            override fun mouseReleased(e: MouseEvent) = release(e)
            override fun mouseMoved(e: MouseEvent) { hover = toPixel(e.point); status(); repaint() }
            override fun mouseExited(e: MouseEvent) { hover = null; status(); repaint() }
            override fun mouseWheelMoved(e: MouseWheelEvent) = wheel(e)
        }
        addMouseListener(mouse)
        addMouseMotionListener(mouse)
        addMouseWheelListener(mouse)
        val im = getInputMap(WHEN_IN_FOCUSED_WINDOW)
        im.put(KeyStroke.getKeyStroke(KeyEvent.VK_SPACE, 0, false), "space-down")
        im.put(KeyStroke.getKeyStroke(KeyEvent.VK_SPACE, 0, true), "space-up")
        actionMap.put("space-down", object : javax.swing.AbstractAction() {
            override fun actionPerformed(e: java.awt.event.ActionEvent?) { spaceDown = true; updateCursor() }
        })
        actionMap.put("space-up", object : javax.swing.AbstractAction() {
            override fun actionPerformed(e: java.awt.event.ActionEvent?) { spaceDown = false; updateCursor() }
        })
        Timer(120) { if (selection != null) { ants = (ants + 1f) % 8f; repaint() } }.start()
        updateCursor()
    }

    private fun updateCursor() {
        cursor = when {
            spaceDown || c.tool == Tool.HAND -> Cursor.getPredefinedCursor(Cursor.HAND_CURSOR)
            c.tool == Tool.MOVE -> Cursor.getPredefinedCursor(Cursor.MOVE_CURSOR)
            else -> Cursor.getPredefinedCursor(Cursor.CROSSHAIR_CURSOR)
        }
    }

    fun refresh() {
        image = c.renderFrame(c.doc.activeFrame, c.onionSkin)
        repaint()
    }

    val zoomPercent: Int get() = (zoom * 100).toInt()

    fun setZoom(z: Double) {
        val cx = width / 2.0; val cy = height / 2.0
        val px = (cx - offset.x) / zoom; val py = (cy - offset.y) / zoom
        zoom = z.coerceIn(1.0, 96.0)
        offset = Point((cx - px * zoom).toInt(), (cy - py * zoom).toInt())
        c.fireTool()
        repaint()
    }

    fun zoomBy(factor: Double) = setZoom(zoom * factor)

    fun fit() {
        val d = c.doc
        if (width <= 0) { centered = false; return }
        zoom = max(1.0, floor(min((width - 64.0) / d.width, (height - 64.0) / d.height)))
        offset = Point(((width - d.width * zoom) / 2).toInt(), ((height - d.height * zoom) / 2).toInt())
        c.fireTool()
        repaint()
    }

    /** Mouse → pixel of the active layer's grid (canvas pixel × density). */
    private fun toPixel(p: Point): Pair<Int, Int> {
        val d = c.activeDensity
        return Pair(floor((p.x - offset.x) / zoom * d).toInt(), floor((p.y - offset.y) / zoom * d).toInt())
    }

    private fun inCanvas(px: Pair<Int, Int>): Boolean {
        val d = c.activeDensity
        return px.first in 0 until c.doc.width * d && px.second in 0 until c.doc.height * d
    }

    private fun status() {
        val h = hover
        if (h == null || !inCanvas(h)) { statusListener(""); return }
        val d = c.activeDensity
        val cx = h.first / d; val cy = h.second / d
        val col = if (cx in 0 until image.width / max(1, image.width / c.doc.width) && image.width > 0) {
            val s = image.width / c.doc.width
            val argb = image.getRGB(min(image.width - 1, h.first * s / d), min(image.height - 1, h.second * s / d))
            if ((argb ushr 24) == 0) "transparent" else String.format("#%06X", argb and 0xFFFFFF)
        } else ""
        statusListener((if (d > 1) "${h.first}, ${h.second} (${d}× grid)" else "$cx, $cy") + "   $col")
    }

    fun regionArgs(): Map<String, Any?> = selection?.let { mapOf("x" to it.x, "y" to it.y, "width" to it.width, "height" to it.height) } ?: emptyMap()

    fun selectAll() { selection = Rectangle(0, 0, c.doc.width, c.doc.height); repaint() }
    fun clearSelection() { selection = null; repaint() }
    fun deleteSelection() { selection?.let { c.run("clear", regionArgs()) } }

    private fun mirrored(px: Pair<Int, Int>): List<Pair<Int, Int>> {
        val d = c.activeDensity
        val w = c.doc.width * d; val h = c.doc.height * d
        val out = mutableListOf(px)
        if (c.mirrorX) out += Pair(w - 1 - px.first, px.second)
        if (c.mirrorY) out += Pair(px.first, h - 1 - px.second)
        if (c.mirrorX && c.mirrorY) out += Pair(w - 1 - px.first, h - 1 - px.second)
        return out
    }

    private fun press(e: MouseEvent) {
        requestFocusInWindow()
        if (SwingUtilities.isMiddleMouseButton(e) || spaceDown || c.tool == Tool.HAND ||
            (SwingUtilities.isRightMouseButton(e) && c.tool !in setOf(Tool.SHADE, Tool.PICKER, Tool.PENCIL))) {
            panStart = e.point
            return
        }
        val px = toPixel(e.point)
        strokeSecondary = SwingUtilities.isRightMouseButton(e)
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER, Tool.SHADE -> {
                if (e.isShiftDown && last != null && c.tool == Tool.PENCIL) { stroke.clear(); addStrokePoint(last!!); addStrokePoint(px) }
                else { stroke.clear(); last = null; addStrokePoint(px) }
            }
            Tool.LINE, Tool.RECTANGLE, Tool.ELLIPSE, Tool.SELECT -> { anchor = px; hover = px }
            Tool.MOVE -> { moveStart = Pair(floor((e.x - offset.x) / zoom).toInt(), floor((e.y - offset.y) / zoom).toInt()); moveDelta = Pair(0, 0) }
            Tool.FILL -> if (inCanvas(px)) c.run("fill_region", mapOf("x" to px.first, "y" to px.second, "color" to (if (strokeSecondary) c.color2 else c.color), "contiguous" to c.contiguous))
            Tool.PICKER -> if (inCanvas(px)) {
                val d = c.activeDensity
                c.run("get_pixel", mapOf("x" to px.first / d, "y" to px.second / d, "composite" to true)).result.str("color")?.let { c.pick(it, strokeSecondary) }
            }
            Tool.HAND -> {}
        }
        repaint()
    }

    private fun addStrokePoint(px: Pair<Int, Int>) {
        val l = last
        if (l == null || stroke.isEmpty()) {
            stroke += mirrored(px)
        } else {
            // Fill gaps between fast mouse samples with a Bresenham run.
            var (x0, y0) = l
            val (x1, y1) = px
            val dx = abs(x1 - x0); val dy = -abs(y1 - y0)
            val sx = if (x0 < x1) 1 else -1; val sy = if (y0 < y1) 1 else -1
            var err = dx + dy
            while (true) {
                stroke += mirrored(Pair(x0, y0))
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
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER, Tool.SHADE -> addStrokePoint(px)
            Tool.MOVE -> moveStart?.let { s -> moveDelta = Pair(floor((e.x - offset.x) / zoom).toInt() - s.first, floor((e.y - offset.y) / zoom).toInt() - s.second) }
            else -> {}
        }
        status()
        repaint()
    }

    /** Removes the middle pixel of L-shaped corners so freehand lines stay one pixel thin. */
    private fun pixelPerfect(path: List<Pair<Int, Int>>): List<Pair<Int, Int>> {
        if (!c.pixelPerfect || c.mirrorX || c.mirrorY) return path
        val out = ArrayList<Pair<Int, Int>>()
        for (p in path) {
            if (out.size >= 2) {
                val a = out[out.size - 2]; val b = out[out.size - 1]
                if (abs(a.first - p.first) == 1 && abs(a.second - p.second) == 1 && (b.first == a.first || b.second == a.second)) out.removeAt(out.size - 1)
            }
            out += p
        }
        return out
    }

    private fun brushed(path: Collection<Pair<Int, Int>>): List<List<Int>> {
        val s = c.brushSize
        if (s <= 1) return path.filter(::inCanvas).map { listOf(it.first, it.second) }
        val set = LinkedHashSet<Pair<Int, Int>>()
        for ((x, y) in path) for (oy in 0 until s) for (ox in 0 until s) set += Pair(x + ox - (s - 1) / 2, y + oy - (s - 1) / 2)
        return set.filter(::inCanvas).map { listOf(it.first, it.second) }
    }

    private fun release(e: MouseEvent) {
        if (panStart != null) { panStart = null; return }
        val px = toPixel(e.point)
        val color = if (strokeSecondary) c.color2 else c.color
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER -> {
                val pixels = brushed(pixelPerfect(stroke.toList()))
                if (pixels.isNotEmpty()) c.run("set_pixels", mapOf("pixels" to pixels, "color" to if (c.tool == Tool.ERASER) null else color))
                stroke.clear()
            }
            Tool.SHADE -> {
                val pixels = brushed(stroke)
                if (pixels.isNotEmpty()) {
                    val r = c.run("shade", mapOf("pixels" to pixels, "steps" to if (strokeSecondary) c.shadeStep else -c.shadeStep))
                    if (!r.ok && c.doc.ramps.isEmpty()) {
                        JOptionPane.showMessageDialog(this, "The Shade brush steps colors along material ramps.\nCreate one in the Ramps panel (+ Ramp) or import a reference with ramps.", "No ramps yet", JOptionPane.INFORMATION_MESSAGE)
                    }
                }
                stroke.clear()
            }
            Tool.LINE -> anchor?.let { a -> c.run("draw_line", mapOf("x0" to a.first, "y0" to a.second, "x1" to px.first, "y1" to px.second, "color" to color)) }
            Tool.RECTANGLE, Tool.ELLIPSE -> anchor?.let { a ->
                val x = min(a.first, px.first); val y = min(a.second, px.second)
                val w = abs(px.first - a.first) + 1; val h = abs(px.second - a.second) + 1
                val op = if (c.tool == Tool.RECTANGLE) "draw_rectangle" else "draw_ellipse"
                c.run(op, mapOf("x" to x, "y" to y, "width" to w, "height" to h, "color" to color, "filled" to (e.isShiftDown || c.filledShapes)))
            }
            Tool.SELECT -> anchor?.let { a ->
                val d = c.activeDensity
                val x0 = min(a.first, px.first) / d; val y0 = min(a.second, px.second) / d
                val x1 = max(a.first, px.first) / d; val y1 = max(a.second, px.second) / d
                selection = Rectangle(x0, y0, x1 - x0 + 1, y1 - y0 + 1).intersection(Rectangle(0, 0, c.doc.width, c.doc.height)).takeIf { !it.isEmpty }
            }
            Tool.MOVE -> {
                val (dx, dy) = moveDelta
                if (dx != 0 || dy != 0) {
                    val d = c.activeDensity
                    val sel = selection ?: Rectangle(0, 0, c.doc.width, c.doc.height)
                    c.run("move_region", mapOf("x" to sel.x * d, "y" to sel.y * d, "width" to sel.width * d, "height" to sel.height * d, "dx" to dx * d, "dy" to dy * d, "mode" to "replace"))
                    selection?.let { selection = Rectangle(it.x + dx, it.y + dy, it.width, it.height) }
                }
                moveStart = null; moveDelta = Pair(0, 0)
            }
            else -> {}
        }
        anchor = null
        repaint()
    }

    /** Wheel zooms through pixel-friendly steps, keeping the pixel under the cursor fixed. */
    private fun wheel(e: MouseWheelEvent) {
        val bx = (e.x - offset.x) / zoom; val by = (e.y - offset.y) / zoom
        val steps = doubleArrayOf(1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0, 64.0, 96.0)
        zoom = if (e.wheelRotation < 0) steps.firstOrNull { it > zoom + 0.01 } ?: 96.0 else steps.lastOrNull { it < zoom - 0.01 } ?: 1.0
        offset = Point((e.x - bx * zoom).toInt(), (e.y - by * zoom).toInt())
        c.fireTool()
        repaint()
    }

    override fun paintComponent(g0: Graphics) {
        super.paintComponent(g0)
        if (!centered && width > 0) { centered = true; fit() }
        val g = g0 as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
        val doc = c.doc
        val w = (doc.width * zoom).toInt(); val h = (doc.height * zoom).toInt()
        // Drop shadow under the canvas.
        g.color = Color(0, 0, 0, 70)
        g.fillRect(offset.x + 4, offset.y + 6, w, h)
        // Transparency checkerboard (fixed 8 px screen cells).
        val clip = g.clip
        g.clipRect(offset.x, offset.y, w, h)
        val cell = 8
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
        val (mdx, mdy) = moveDelta
        g.drawImage(image, offset.x + (mdx * zoom).toInt(), offset.y + (mdy * zoom).toInt(), w, h, null)
        // Stroke / shape preview in layer-grid units.
        val d = c.activeDensity
        val cz = zoom / d
        val previewColor = when (c.tool) {
            Tool.ERASER -> Color(255, 255, 255, 110)
            Tool.SHADE -> if (strokeSecondary) Color(255, 240, 200, 110) else Color(20, 10, 40, 120)
            else -> Theme.parse(if (strokeSecondary) c.color2 else c.color)
        }
        g.color = previewColor
        val size = c.brushSize
        for ((x, y) in stroke) g.fillRect(offset.x + ((x - (size - 1) / 2) * cz).toInt(), offset.y + ((y - (size - 1) / 2) * cz).toInt(), max(1, (cz * size).toInt()), max(1, (cz * size).toInt()))
        val a = anchor; val hv = hover
        if (a != null && hv != null) {
            g.stroke = BasicStroke(1f)
            val x = min(a.first, hv.first); val y = min(a.second, hv.second)
            val rw = abs(hv.first - a.first) + 1; val rh = abs(hv.second - a.second) + 1
            val sx = offset.x + (x * cz).toInt(); val sy = offset.y + (y * cz).toInt()
            when (c.tool) {
                Tool.LINE -> g.drawLine(offset.x + ((a.first + 0.5) * cz).toInt(), offset.y + ((a.second + 0.5) * cz).toInt(), offset.x + ((hv.first + 0.5) * cz).toInt(), offset.y + ((hv.second + 0.5) * cz).toInt())
                Tool.RECTANGLE -> g.drawRect(sx, sy, (rw * cz).toInt(), (rh * cz).toInt())
                Tool.ELLIPSE -> g.drawOval(sx, sy, (rw * cz).toInt(), (rh * cz).toInt())
                Tool.SELECT -> { g.color = Theme.ACCENT; g.drawRect(sx, sy, (rw * cz).toInt(), (rh * cz).toInt()) }
                else -> {}
            }
        }
        // Pixel grid of the active layer (fine grid on density layers), then the canvas pixel grid.
        if (c.showGrid && cz >= 6) {
            g.color = Theme.GRID_FINE
            val gw = doc.width * d; val gh = doc.height * d
            for (x in 0..gw) g.drawLine(offset.x + (x * cz).toInt(), offset.y, offset.x + (x * cz).toInt(), offset.y + h)
            for (y in 0..gh) g.drawLine(offset.x, offset.y + (y * cz).toInt(), offset.x + w, offset.y + (y * cz).toInt())
        }
        if (c.showGrid && zoom >= 6 && d > 1) {
            g.color = Theme.GRID
            for (x in 0..doc.width) g.drawLine(offset.x + (x * zoom).toInt(), offset.y, offset.x + (x * zoom).toInt(), offset.y + h)
            for (y in 0..doc.height) g.drawLine(offset.x, offset.y + (y * zoom).toInt(), offset.x + w, offset.y + (y * zoom).toInt())
        }
        // Symmetry guides.
        g.stroke = BasicStroke(1f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER, 10f, floatArrayOf(6f, 4f), 0f)
        g.color = Theme.ACCENT_2
        if (c.mirrorX) g.drawLine(offset.x + w / 2, offset.y, offset.x + w / 2, offset.y + h)
        if (c.mirrorY) g.drawLine(offset.x, offset.y + h / 2, offset.x + w, offset.y + h / 2)
        // Brush cursor.
        if (hv != null && inCanvas(hv) && c.tool !in setOf(Tool.HAND, Tool.MOVE, Tool.SELECT)) {
            g.stroke = BasicStroke(1f)
            val bs = if (c.tool in setOf(Tool.PENCIL, Tool.ERASER, Tool.SHADE)) size else 1
            val bx = offset.x + ((hv.first - (bs - 1) / 2) * cz).toInt(); val by = offset.y + ((hv.second - (bs - 1) / 2) * cz).toInt()
            g.color = Color(0, 0, 0, 160)
            g.drawRect(bx - 1, by - 1, max(1, (cz * bs).toInt()) + 1, max(1, (cz * bs).toInt()) + 1)
            g.color = Color.WHITE
            g.drawRect(bx, by, max(1, (cz * bs).toInt()) - 1, max(1, (cz * bs).toInt()) - 1)
        }
        g.clip = clip
        // Selection with marching ants.
        selection?.let { s ->
            val r = Rectangle(offset.x + ((s.x + mdx) * zoom).toInt(), offset.y + ((s.y + mdy) * zoom).toInt(), (s.width * zoom).toInt(), (s.height * zoom).toInt())
            g.stroke = BasicStroke(1f)
            g.color = Color.BLACK
            g.drawRect(r.x, r.y, r.width, r.height)
            g.stroke = BasicStroke(1f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER, 10f, floatArrayOf(4f, 4f), ants)
            g.color = Color.WHITE
            g.drawRect(r.x, r.y, r.width, r.height)
        }
        g.stroke = BasicStroke(1f)
        g.color = Theme.BORDER
        g.drawRect(offset.x - 1, offset.y - 1, w + 1, h + 1)
    }
}

// ═══ OptionsBar / StatusBar ══════════════════════════════════════════════════════════════════════

/** Context bar under the menu: options of the active tool, symmetry, view toggles and zoom. */
class OptionsBar(private val c: EditorController, private val canvas: CanvasView) : JPanel(BorderLayout()) {
    private val left = JPanel(FlowLayout(FlowLayout.LEFT, 6, 6)).apply { isOpaque = false }
    private val right = JPanel(FlowLayout(FlowLayout.RIGHT, 6, 6)).apply { isOpaque = false }
    private val zoomLabel = JLabel().apply { foreground = Theme.TEXT_DIM; preferredSize = Dimension(56, 20); horizontalAlignment = SwingConstants.RIGHT }

    init {
        background = Theme.PANEL
        border = BorderFactory.createMatteBorder(0, 0, 1, 0, Theme.BORDER)
        add(left, BorderLayout.CENTER)
        add(right, BorderLayout.EAST)
        right.add(Theme.chipToggle("Grid", c.showGrid, "Pixel grid (Ctrl+')") { c.showGrid = it; canvas.repaint() })
        right.add(Theme.chipToggle("Onion", c.onionSkin, "Onion skin: neighbouring frames") { c.onionSkin = it; canvas.refresh() })
        right.add(Theme.separatorV())
        right.add(Theme.flatButton("−", "Zoom out") { canvas.zoomBy(0.8) })
        right.add(zoomLabel)
        right.add(Theme.flatButton("+", "Zoom in") { canvas.zoomBy(1.25) })
        right.add(Theme.flatButton("Fit", "Fit to window (0)") { canvas.fit() })
        c.onToolChange { rebuild() }
        rebuild()
    }

    private fun label(t: String) = JLabel(t).apply { foreground = Theme.TEXT_DIM }

    private fun rebuild() {
        zoomLabel.text = "${canvas.zoomPercent}%"
        left.removeAll()
        left.add(JLabel(ToolIcon(c.tool, 16)).apply { border = BorderFactory.createEmptyBorder(0, 6, 0, 0) })
        left.add(JLabel(c.tool.label).apply { foreground = Theme.TEXT; font = font.deriveFont(Font.BOLD) })
        left.add(Theme.separatorV())
        when (c.tool) {
            Tool.PENCIL, Tool.ERASER, Tool.SHADE -> {
                left.add(label("Size"))
                left.add(Theme.segmented(listOf("1", "2", "3", "4"), c.brushSize - 1) { c.brushSize = it + 1 })
                if (c.tool == Tool.PENCIL) left.add(Theme.chipToggle("Pixel perfect", c.pixelPerfect, "Removes L-shaped corner doubles from strokes") { c.pixelPerfect = it })
                if (c.tool == Tool.SHADE) {
                    left.add(label("Step"))
                    left.add(Theme.segmented(listOf("1", "2", "3"), c.shadeStep - 1) { c.shadeStep = it + 1 })
                    left.add(label("Left click darker · right click lighter · only colors of a ramp change"))
                }
            }
            Tool.RECTANGLE, Tool.ELLIPSE -> left.add(Theme.chipToggle("Filled", c.filledShapes, "Fill shapes (or hold Shift)") { c.filledShapes = it })
            Tool.FILL -> left.add(Theme.chipToggle("Contiguous", c.contiguous, "Off: replace that color everywhere on the layer") { c.contiguous = it })
            Tool.SELECT, Tool.MOVE -> left.add(label("Delete clears · Esc deselects · Edit menu flips/rotates the selection"))
            Tool.PICKER -> left.add(label("Left click: primary · right click: secondary"))
            else -> left.add(label("Drag to pan · wheel zooms at the cursor"))
        }
        if (c.tool in setOf(Tool.PENCIL, Tool.ERASER, Tool.SHADE, Tool.LINE, Tool.RECTANGLE, Tool.ELLIPSE)) {
            left.add(Theme.separatorV())
            left.add(label("Mirror"))
            left.add(Theme.chipToggle("X", c.mirrorX, "Horizontal symmetry") { c.mirrorX = it; canvas.repaint() })
            left.add(Theme.chipToggle("Y", c.mirrorY, "Vertical symmetry") { c.mirrorY = it; canvas.repaint() })
        }
        left.revalidate(); left.repaint()
    }
}

class StatusBar(private val c: EditorController, private val gateway: AgentGateway) : JPanel(BorderLayout()) {
    var cursor = ""
    private val leftLabel = JLabel()
    private val rightLabel = JLabel()
    private var flashText: String? = null
    private val flashTimer = Timer(4000) { flashText = null; refresh() }.apply { isRepeats = false }

    init {
        background = Theme.BG
        border = BorderFactory.createCompoundBorder(BorderFactory.createMatteBorder(1, 0, 0, 0, Theme.BORDER), BorderFactory.createEmptyBorder(4, 12, 4, 12))
        leftLabel.foreground = Theme.TEXT_DIM
        rightLabel.foreground = Theme.TEXT_DIM
        add(leftLabel, BorderLayout.CENTER)
        add(rightLabel, BorderLayout.EAST)
        refresh()
    }

    fun flash(text: String) { flashText = text; flashTimer.restart(); refresh() }

    fun refresh() {
        val d = c.doc
        val layer = d.layers.getOrNull(d.activeLayer)
        val dens = if ((layer?.density ?: 1) > 1) " · ${layer!!.density}× pixels" else ""
        val err = c.lastError?.let { "   ⚠ $it" } ?: ""
        val pos = if (cursor.isNotBlank()) "   ·   $cursor" else ""
        leftLabel.text = flashText ?: "${d.width} × ${d.height}   ·   ${layer?.name ?: ""}$dens   ·   frame ${d.activeFrame + 1}/${d.frames.size}   ·   ${d.ramps.size} ramps$pos$err"
        leftLabel.foreground = if (c.lastError != null && flashText == null) Theme.WARN else Theme.TEXT_DIM
        val agent = if (gateway.running) "● Agent Mode  ${gateway.mcpUrl}" else "○ Agent Mode off"
        rightLabel.text = "rev ${d.revision}     $agent"
        rightLabel.foreground = if (gateway.running) Theme.OK else Theme.TEXT_DIM
    }
}

// ═══ Color ═══════════════════════════════════════════════════════════════════════════════════════

/** Primary/secondary color chips under the tool rail (click opens the picker tab, ⇄ swaps). */
class ColorChips(private val c: EditorController) : JComponent() {
    init {
        preferredSize = Dimension(40, 52); maximumSize = preferredSize; alignmentX = CENTER_ALIGNMENT
        toolTipText = "Primary / secondary color — X swaps"
        c.onToolChange { repaint() }
        addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                if (e.x > 26 && e.y < 14) { c.swapColors(); return }
                val secondary = e.x > 16 && e.y > 20
                JColorChooser.showDialog(this@ColorChips, if (secondary) "Secondary color" else "Primary color", Theme.parse(if (secondary) c.color2 else c.color))?.let {
                    c.pick(Theme.hex(it), secondary)
                }
            }
        })
    }

    override fun paintComponent(g0: Graphics) {
        val g = g0 as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
        fun chip(x: Int, y: Int, hex: String) {
            g.color = Theme.BORDER; g.fillRoundRect(x - 1, y - 1, 26, 26, 6, 6)
            g.color = Theme.parse(hex); g.fillRoundRect(x, y, 24, 24, 5, 5)
        }
        chip(14, 24, c.color2)
        chip(2, 12, c.color)
        // swap arrows (top right)
        g.color = Theme.TEXT_DIM
        g.stroke = BasicStroke(1.3f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND)
        g.drawArc(26, 2, 10, 10, 0, 90)
        g.drawPolyline(intArrayOf(29, 31, 29), intArrayOf(0, 2, 4), 3)
        g.drawPolyline(intArrayOf(34, 36, 38), intArrayOf(5, 7, 5), 3)
    }
}

/** HSV picker: saturation/value square, hue strip and a hex field. Sets the primary color. */
class ColorPicker(private val c: EditorController) : JPanel(BorderLayout(8, 8)) {
    private var hsv = FloatArray(3)
    private val hexField = JTextField(8)
    private var updating = false

    private val square = object : JComponent() {
        init {
            preferredSize = Dimension(200, 110)
            val m = object : MouseAdapter() {
                override fun mousePressed(e: MouseEvent) = set(e)
                override fun mouseDragged(e: MouseEvent) = set(e)
                fun set(e: MouseEvent) {
                    hsv[1] = (e.x.toFloat() / width).coerceIn(0f, 1f)
                    hsv[2] = 1f - (e.y.toFloat() / height).coerceIn(0f, 1f)
                    apply()
                }
            }
            addMouseListener(m); addMouseMotionListener(m)
        }

        override fun paintComponent(g0: Graphics) {
            val g = g0 as Graphics2D
            g.paint = GradientPaint(0f, 0f, Color.WHITE, width.toFloat(), 0f, Color.getHSBColor(hsv[0], 1f, 1f))
            g.fillRect(0, 0, width, height)
            g.paint = GradientPaint(0f, 0f, Color(0, 0, 0, 0), 0f, height.toFloat(), Color.BLACK)
            g.fillRect(0, 0, width, height)
            val x = (hsv[1] * width).toInt(); val y = ((1 - hsv[2]) * height).toInt()
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            g.color = Color.BLACK; g.drawOval(x - 6, y - 6, 12, 12)
            g.color = Color.WHITE; g.drawOval(x - 5, y - 5, 10, 10)
        }
    }

    private val hue = object : JComponent() {
        init {
            preferredSize = Dimension(200, 14)
            val m = object : MouseAdapter() {
                override fun mousePressed(e: MouseEvent) = set(e)
                override fun mouseDragged(e: MouseEvent) = set(e)
                fun set(e: MouseEvent) { hsv[0] = (e.x.toFloat() / width).coerceIn(0f, 0.999f); apply() }
            }
            addMouseListener(m); addMouseMotionListener(m)
        }

        override fun paintComponent(g: Graphics) {
            for (x in 0 until width) { g.color = Color.getHSBColor(x.toFloat() / width, 1f, 1f); g.drawLine(x, 0, x, height) }
            val x = (hsv[0] * width).toInt()
            g.color = Color.WHITE; g.fillRect(x - 1, 0, 3, height)
            g.color = Color.BLACK; g.drawRect(x - 2, 0, 4, height - 1)
        }
    }

    init {
        isOpaque = false
        border = BorderFactory.createEmptyBorder(4, 0, 4, 0)
        add(square, BorderLayout.CENTER)
        val south = JPanel(BorderLayout(6, 6)).apply { isOpaque = false }
        south.add(hue, BorderLayout.NORTH)
        south.add(JPanel(FlowLayout(FlowLayout.LEFT, 0, 0)).apply {
            isOpaque = false
            add(JLabel("HEX  ").apply { foreground = Theme.TEXT_DIM })
            add(Theme.field(hexField))
        }, BorderLayout.CENTER)
        add(south, BorderLayout.SOUTH)
        hexField.addActionListener { runCatching { Theme.parse(hexField.text.trim().let { if (it.startsWith("#")) it else "#$it" }) }.getOrNull()?.let { c.pick(Theme.hex(it)) } }
        c.onToolChange { sync() }
        sync()
    }

    private fun sync() {
        if (updating) return
        val col = Theme.parse(c.color)
        hsv = Color.RGBtoHSB(col.red, col.green, col.blue, null)
        hexField.text = c.color
        repaint()
    }

    private fun apply() {
        updating = true
        c.pick(Theme.hex(Color.getHSBColor(hsv[0], hsv[1], hsv[2])))
        hexField.text = c.color
        updating = false
        repaint()
    }
}

/** Material ramps (dark → light rows) and the free swatch list. */
class RampsPanel(private val c: EditorController) : JPanel() {
    init {
        layout = BoxLayout(this, BoxLayout.Y_AXIS)
        isOpaque = false
        c.onChange { rebuild() }
        c.onToolChange { repaint() }
        rebuild()
    }

    private fun swatch(hex: String, tip: String): JComponent = object : JComponent() {
        init {
            preferredSize = Dimension(22, 22); minimumSize = preferredSize; maximumSize = preferredSize
            toolTipText = tip
            addMouseListener(object : MouseAdapter() {
                override fun mouseClicked(e: MouseEvent) = c.pick(hex, SwingUtilities.isRightMouseButton(e))
            })
        }

        override fun paintComponent(g: Graphics) {
            g.color = Theme.parse(hex); g.fillRect(1, 1, width - 2, height - 2)
            if (hex.equals(c.color, true)) { g.color = Color.WHITE; g.drawRect(0, 0, width - 1, height - 1); g.color = Color.BLACK; g.drawRect(1, 1, width - 3, height - 3) }
        }
    }

    private fun rebuild() {
        removeAll()
        val ramps = c.doc.ramps
        if (ramps.isEmpty()) {
            add(Theme.hint("No material ramps yet. A ramp is one material's tones, dark → light; the Shade brush and auto shading step along it."))
        }
        for (r in ramps) {
            add(JPanel(BorderLayout(6, 0)).apply {
                isOpaque = false
                maximumSize = Dimension(Int.MAX_VALUE, 26)
                add(JLabel(r.name).apply { foreground = Theme.TEXT_DIM; preferredSize = Dimension(70, 20); font = font.deriveFont(11.5f) }, BorderLayout.WEST)
                add(JPanel(FlowLayout(FlowLayout.LEFT, 0, 0)).apply { isOpaque = false; r.colors.forEach { add(swatch(it, "${r.name}: $it")) } }, BorderLayout.CENTER)
            })
            add(Box.createVerticalStrut(2))
        }
        add(Box.createVerticalStrut(6))
        add(JPanel(FlowLayout(FlowLayout.LEFT, 4, 0)).apply {
            isOpaque = false
            maximumSize = Dimension(Int.MAX_VALUE, 32)
            add(Theme.flatButton("+ Ramp", "Build a 5-tone hue-shifted ramp from the primary color") {
                val name = JOptionPane.showInputDialog(this, "Ramp name (skin, hair, cloth, metal…)", "material") ?: return@flatButton
                if (name.isNotBlank()) c.run("color_ramp", mapOf("base" to c.color, "steps" to 5, "name" to name.trim()))
            })
            add(Theme.flatButton("+ Swatch", "Add the primary color to the swatches") { c.run("palette_set", mapOf("colors" to listOf(c.color), "mode" to "add")) })
        })
        add(Box.createVerticalStrut(8))
        val grid = JPanel(GridLayout(0, 10, 0, 0)).apply { isOpaque = false }
        c.doc.palette.forEach { grid.add(swatch(it, it)) }
        add(JPanel(BorderLayout()).apply { isOpaque = false; add(grid, BorderLayout.NORTH); maximumSize = Dimension(Int.MAX_VALUE, 22 * ((c.doc.palette.size + 9) / 10) + 4) })
        revalidate(); repaint()
    }
}

// ═══ Layers ══════════════════════════════════════════════════════════════════════════════════════

/** Layer rows (top of the stack first) with visibility, lock, thumbnail, density badge and properties. */
class LayersPanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val rows = JPanel().apply { layout = BoxLayout(this, BoxLayout.Y_AXIS); background = Theme.PANEL }
    private val opacity = JSpinner(SpinnerNumberModel(100, 0, 100, 5))
    private val blend = Theme.combo(arrayOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light"))
    private val density = Theme.combo(arrayOf("1× pixels", "2× (half-size)", "4× (quarter-size)"))
    private var updating = false
    private val thumbs = HashMap<Int, BufferedImage>()
    private var thumbRev = -1L

    init {
        isOpaque = false
        add(JScrollPane(rows).apply {
            border = BorderFactory.createLineBorder(Theme.BORDER)
            preferredSize = Dimension(270, 220)
            Theme.scroll(this)
        }, BorderLayout.CENTER)
        val props = JPanel(GridLayout(0, 2, 6, 6)).apply {
            isOpaque = false
            border = BorderFactory.createEmptyBorder(8, 0, 4, 0)
            add(Theme.small("Opacity %")); add(opacity)
            add(Theme.small("Blend")); add(blend)
            add(Theme.small("Pixel size")); add(density)
        }
        opacity.addChangeListener { if (!updating) c.run("layer_set", mapOf("layer" to c.doc.activeLayer, "opacity" to ((opacity.value as Int) * 255 + 50) / 100)) }
        blend.addActionListener { if (!updating) c.run("layer_set", mapOf("layer" to c.doc.activeLayer, "blend" to blend.selectedItem)) }
        density.addActionListener {
            if (updating) return@addActionListener
            val d = listOf(1, 2, 4)[density.selectedIndex]
            if (d != c.activeDensity) c.run("layer_set", mapOf("layer" to c.doc.activeLayer, "density" to d))
        }
        val buttons = JPanel(FlowLayout(FlowLayout.LEFT, 4, 0)).apply {
            isOpaque = false
            add(Theme.flatButton("+ Layer", "New layer above the active one") { c.run("layer_create") })
            add(Theme.flatButton("Delete", "Delete the active layer in every frame") {
                val l = c.doc.layers.getOrNull(c.doc.activeLayer) ?: return@flatButton
                if (JOptionPane.showConfirmDialog(this, "Delete layer '${l.name}' in every frame?", "Delete layer", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                    c.run("layer_delete", mapOf("layer" to l.index))
                }
            })
            add(Theme.flatButton("▲", "Move up") { val i = c.doc.activeLayer; if (i < c.doc.layers.size - 1) c.run("layer_move", mapOf("layer" to i, "index" to i + 1)) })
            add(Theme.flatButton("▼", "Move down") { val i = c.doc.activeLayer; if (i > 0) c.run("layer_move", mapOf("layer" to i, "index" to i - 1)) })
        }
        add(JPanel(BorderLayout()).apply { isOpaque = false; add(props, BorderLayout.CENTER); add(buttons, BorderLayout.SOUTH) }, BorderLayout.SOUTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun thumb(l: LayerInfo): BufferedImage {
        if (thumbRev != c.doc.revision) { thumbs.clear(); thumbRev = c.doc.revision }
        return thumbs.getOrPut(l.index) { c.renderFrame(c.doc.activeFrame, layer = l.index, density = 1) }
    }

    private inner class Row(val l: LayerInfo, val active: Boolean) : JPanel(BorderLayout(6, 0)) {
        init {
            background = if (active) Theme.SELECTED else Theme.PANEL
            border = BorderFactory.createCompoundBorder(BorderFactory.createMatteBorder(0, 0, 1, 0, Theme.BORDER_SOFT), BorderFactory.createEmptyBorder(3, 6, 3, 8))
            maximumSize = Dimension(Int.MAX_VALUE, 40)
            val icons = JPanel(FlowLayout(FlowLayout.LEFT, 2, 4)).apply {
                isOpaque = false
                add(Theme.iconButton(GlyphIcon(if (l.visible) GlyphIcon.Kind.EYE else GlyphIcon.Kind.EYE_OFF), if (l.visible) "Hide" else "Show") {
                    c.run("layer_set", mapOf("layer" to l.index, "visible" to !l.visible))
                })
                add(Theme.iconButton(GlyphIcon(if (l.locked) GlyphIcon.Kind.LOCK else GlyphIcon.Kind.UNLOCK), if (l.locked) "Unlock" else "Lock") {
                    c.run("layer_set", mapOf("layer" to l.index, "locked" to !l.locked))
                })
            }
            add(icons, BorderLayout.WEST)
            val t = thumb(l)
            val thumbIcon = object : Icon {
                override fun getIconWidth() = 30
                override fun getIconHeight() = 30
                override fun paintIcon(cmp: Component?, g0: Graphics, x: Int, y: Int) {
                    val g = g0 as Graphics2D
                    g.color = Theme.CHECKER_A; g.fillRect(x, y, 30, 30)
                    g.color = Theme.CHECKER_B; g.fillRect(x, y, 15, 15); g.fillRect(x + 15, y + 15, 15, 15)
                    val s = min(30.0 / t.width, 30.0 / t.height)
                    val w = (t.width * s).toInt(); val h = (t.height * s).toInt()
                    g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_BILINEAR)
                    g.drawImage(t, x + (30 - w) / 2, y + (30 - h) / 2, w, h, null)
                    g.color = Theme.BORDER; g.drawRect(x, y, 29, 29)
                }
            }
            val name = JLabel(l.name, thumbIcon, SwingConstants.LEFT).apply {
                iconTextGap = 8
                foreground = if (l.visible) Theme.TEXT else Theme.TEXT_DIM
                if (active) font = font.deriveFont(Font.BOLD)
            }
            add(name, BorderLayout.CENTER)
            val badges = buildList {
                if (l.density > 1) add("${l.density}×")
                if (l.blend != "normal") add(l.blend)
                if (l.opacity < 255) add("${l.opacity * 100 / 255}%")
            }
            if (badges.isNotEmpty()) add(JLabel(badges.joinToString("  ")).apply { foreground = Theme.ACCENT; font = font.deriveFont(11f) }, BorderLayout.EAST)
            val click = object : MouseAdapter() {
                override fun mouseClicked(e: MouseEvent) {
                    if (e.clickCount >= 2) {
                        val n = JOptionPane.showInputDialog(this@LayersPanel, "Layer name", l.name) ?: return
                        if (n.isNotBlank()) c.run("layer_set", mapOf("layer" to l.index, "name" to n.trim()))
                    } else if (!active) c.run("layer_select", mapOf("layer" to l.index))
                }
            }
            addMouseListener(click); name.addMouseListener(click)
        }
    }

    private fun rebuild() {
        updating = true
        rows.removeAll()
        c.doc.layers.reversed().forEach { rows.add(Row(it, it.index == c.doc.activeLayer)) }
        rows.add(Box.createVerticalGlue())
        c.doc.layers.getOrNull(c.doc.activeLayer)?.let {
            opacity.value = (it.opacity * 100 + 127) / 255
            blend.selectedItem = it.blend
            density.selectedIndex = when (it.density) { 2 -> 1; 4 -> 2; else -> 0 }
        }
        rows.revalidate(); rows.repaint()
        updating = false
    }
}

/** Live list of what connected agents have been doing. */
class ActivityPanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val model = DefaultListModel<String>()

    init {
        isOpaque = false
        val list = JList(model).apply {
            foreground = Theme.TEXT_DIM; background = Theme.PANEL; font = Font(Font.MONOSPACED, Font.PLAIN, 11)
            selectionBackground = Theme.SELECTED
        }
        add(JScrollPane(list).apply { preferredSize = Dimension(270, 110); border = BorderFactory.createLineBorder(Theme.BORDER); Theme.scroll(this) })
        c.onChange { rebuild() }
        rebuild()
    }

    private fun rebuild() {
        model.clear()
        c.recentAgentActivity().forEach { model.addElement(it) }
        if (model.isEmpty) model.addElement("No agent edits yet")
    }
}

/** Right-hand dock: collapsible sections for color, ramps & swatches, layers and agent activity. */
class Dock(c: EditorController) : JPanel(BorderLayout()) {
    init {
        background = Theme.PANEL
        border = BorderFactory.createMatteBorder(0, 1, 0, 0, Theme.BORDER)
        preferredSize = Dimension(300, 400)
        fun column(vararg parts: JComponent) = JPanel().apply {
            layout = BoxLayout(this, BoxLayout.Y_AXIS); background = Theme.PANEL; border = BorderFactory.createEmptyBorder(0, 12, 8, 12)
            parts.forEach { add(it) }
            add(Box.createVerticalGlue())
        }
        fun scroller(p: JComponent) = JScrollPane(p).apply { border = BorderFactory.createEmptyBorder(); horizontalScrollBarPolicy = JScrollPane.HORIZONTAL_SCROLLBAR_NEVER; Theme.scroll(this) }
        val top = scroller(column(Theme.collapsible("COLOR", ColorPicker(c), true), Theme.collapsible("RAMPS & SWATCHES", RampsPanel(c), true)))
        val bottom = scroller(column(Theme.collapsible("LAYERS", LayersPanel(c), true), Theme.collapsible("AGENT ACTIVITY", ActivityPanel(c), false)))
        val split = javax.swing.JSplitPane(javax.swing.JSplitPane.VERTICAL_SPLIT, top, bottom).apply {
            setUI(object : javax.swing.plaf.basic.BasicSplitPaneUI() {
                override fun createDefaultDivider() = object : javax.swing.plaf.basic.BasicSplitPaneDivider(this) {
                    override fun paint(g: Graphics) { g.color = Theme.PANEL; g.fillRect(0, 0, width, height); g.color = Theme.BORDER; g.fillRect(0, height / 2, width, 1) }
                }
            })
            border = BorderFactory.createEmptyBorder(); dividerSize = 7; resizeWeight = 0.5; isContinuousLayout = true
        }
        split.addComponentListener(object : java.awt.event.ComponentAdapter() {
            var done = false
            override fun componentResized(e: java.awt.event.ComponentEvent) { if (!done && split.height > 0) { done = true; split.setDividerLocation(0.52) } }
        })
        add(split, BorderLayout.CENTER)
    }
}

// ═══ TimelinePanel ═══════════════════════════════════════════════════════════════════════════════

/** Frame strip with thumbnails, per-frame durations and playback. */
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
    private val play = Theme.flatButton("▶  Play", "Play / stop") { togglePlay() }

    init {
        background = Theme.PANEL
        border = BorderFactory.createMatteBorder(1, 0, 0, 0, Theme.BORDER)
        list.layoutOrientation = JList.HORIZONTAL_WRAP
        list.visibleRowCount = 1
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.fixedCellWidth = 72
        list.fixedCellHeight = 76
        list.background = Theme.PANEL
        list.cellRenderer = ListCellRenderer<FrameInfo> { _, f, _, selected, _ -> Cell(f, selected) as Component }
        list.addListSelectionListener {
            if (!updating && !it.valueIsAdjusting) list.selectedValue?.let { f -> c.run("frame_select", mapOf("frame" to f.index)) }
        }
        add(JScrollPane(list).apply { preferredSize = Dimension(600, 84); border = BorderFactory.createEmptyBorder(); Theme.scroll(this) }, BorderLayout.CENTER)

        val controls = JPanel(FlowLayout(FlowLayout.LEFT, 6, 6)).apply {
            isOpaque = false
            add(JLabel("TIMELINE").apply { foreground = Theme.TEXT_DIM; font = font.deriveFont(Font.BOLD, 11f); border = BorderFactory.createEmptyBorder(0, 6, 0, 8) })
            add(play)
            add(Theme.flatButton("+ Frame", "Add an empty frame after the current one") { c.run("frame_add", mapOf("index" to c.doc.activeFrame + 1)) })
            add(Theme.flatButton("Duplicate", "Duplicate the current frame") { c.run("frame_duplicate", mapOf("frame" to c.doc.activeFrame)) })
            add(Theme.flatButton("Delete", "Delete the current frame") {
                if (c.doc.frames.size > 1 && JOptionPane.showConfirmDialog(this, "Delete frame ${c.doc.activeFrame + 1}?", "Delete frame", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                    c.run("frame_delete", mapOf("frame" to c.doc.activeFrame))
                }
            })
            add(Theme.flatButton("◀", "Move frame left") { if (c.doc.activeFrame > 0) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame - 1)) })
            add(Theme.flatButton("▶", "Move frame right") { if (c.doc.activeFrame < c.doc.frames.size - 1) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame + 1)) })
            add(Theme.separatorV())
            add(JLabel("Duration").apply { foreground = Theme.TEXT_DIM })
            add(duration.apply { preferredSize = Dimension(76, 26) })
            add(JLabel("ms").apply { foreground = Theme.TEXT_DIM })
            duration.addChangeListener { if (!updating) c.run("frame_set_duration", mapOf("frame" to c.doc.activeFrame, "duration" to duration.value)) }
            add(Theme.separatorV())
            add(info.apply { foreground = Theme.TEXT_DIM })
        }
        add(controls, BorderLayout.NORTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun thumb(index: Int): BufferedImage {
        if (thumbRevision != c.doc.revision) { thumbs.clear(); thumbRevision = c.doc.revision }
        return thumbs.getOrPut(index) { c.renderFrame(index, density = 1) }
    }

    private inner class Cell(val f: FrameInfo, val selected: Boolean) : JPanel() {
        init { preferredSize = Dimension(72, 76); isOpaque = true }

        override fun paintComponent(g0: Graphics) {
            val g = g0 as Graphics2D
            g.color = Theme.PANEL
            g.fillRect(0, 0, width, height)
            val img = thumb(f.index)
            val box = 52
            val s = minOf(box.toDouble() / img.width, box.toDouble() / img.height)
            val w = (img.width * s).toInt(); val h = (img.height * s).toInt()
            val x = (width - w) / 2; val y = 4 + (box - h) / 2
            g.color = Theme.CHECKER_B
            g.fillRect(x, y, w, h)
            g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
            g.drawImage(img, x, y, w, h, null)
            g.color = if (selected) Theme.ACCENT else Theme.BORDER
            g.stroke = BasicStroke(if (selected) 2f else 1f)
            g.drawRect(x - 1, y - 1, w + 1, h + 1)
            g.color = if (selected) Theme.TEXT else Theme.TEXT_DIM
            g.font = g.font.deriveFont(11f)
            g.drawString("${f.index + 1}", 6, height - 6)
            val ms = "${f.duration}ms"
            g.drawString(ms, width - 6 - g.fontMetrics.stringWidth(ms), height - 6)
        }
    }

    private fun togglePlay() {
        timer?.let { it.stop(); timer = null; play.text = "▶  Play"; return }
        play.text = "■  Stop"
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
        info.text = "${c.doc.frames.size} frames  ·  ${c.doc.totalMs} ms  ·  ${"%.1f".format(c.doc.fps)} fps"
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
        add(JScrollPane(controls()).apply {
            preferredSize = Dimension(300, 600); border = BorderFactory.createMatteBorder(0, 1, 0, 0, Theme.BORDER)
            horizontalScrollBarPolicy = JScrollPane.HORIZONTAL_SCROLLBAR_NEVER
        }, BorderLayout.EAST)
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

/**
 * Flat dark theme in the spirit of Windows 11 (Fluent): neutral greys so pixel colors read
 * true, one blue accent, 6 px rounded controls, thin scrollbars, Segoe UI Variable when present.
 */
object Theme {
    val BG = Color(0x1B, 0x1B, 0x1E)
    val PANEL = Color(0x23, 0x23, 0x27)
    val PANEL_LIGHT = Color(0x2D, 0x2D, 0x32)
    val HOVER = Color(0x34, 0x34, 0x3A)
    val PRESSED = Color(0x3C, 0x3C, 0x44)
    val SELECTED = Color(0x2C, 0x36, 0x4D)
    val BORDER = Color(0x37, 0x37, 0x3D)
    val BORDER_SOFT = Color(0x2B, 0x2B, 0x30)
    val CANVAS_BG = Color(0x16, 0x16, 0x19)
    val TEXT = Color(0xEC, 0xEC, 0xEF)
    val TEXT_DIM = Color(0x9C, 0x9C, 0xA6)
    val ACCENT = Color(0x5B, 0x9B, 0xFF)
    val ACCENT_2 = Color(0xC0, 0x8C, 0xFF)
    val ACCENT_DIM = Color(0x5B, 0x9B, 0xFF, 90)
    val OK = Color(0x5C, 0xD1, 0x8C)
    val WARN = Color(0xFF, 0xB0, 0x57)
    val GRID = Color(255, 255, 255, 34)
    val GRID_FINE = Color(255, 255, 255, 16)
    val CHECKER_A = Color(0x3A, 0x3A, 0x40)
    val CHECKER_B = Color(0x30, 0x30, 0x35)

    val FONT: Font by lazy {
        val names = GraphicsEnvironment.getLocalGraphicsEnvironment().availableFontFamilyNames.toSet()
        val family = listOf("Segoe UI Variable Text", "Segoe UI", "Inter", "SF Pro Text", "Noto Sans", "DejaVu Sans").firstOrNull { it in names } ?: Font.SANS_SERIF
        Font(family, Font.PLAIN, 13)
    }

    fun install() {
        runCatching { UIManager.setLookAndFeel("javax.swing.plaf.metal.MetalLookAndFeel") }
        val f = javax.swing.plaf.FontUIResource(FONT)
        UIManager.getDefaults().keys.toList().forEach { k -> if (k.toString().endsWith(".font")) UIManager.put(k, f) }
        fun put(vararg kv: Pair<String, Any>) = kv.forEach { (k, v) -> UIManager.put(k, v) }
        put(
            "Panel.background" to PANEL, "Label.foreground" to TEXT, "OptionPane.background" to PANEL, "OptionPane.messageForeground" to TEXT,
            "MenuBar.background" to BG, "MenuBar.border" to BorderFactory.createMatteBorder(0, 0, 1, 0, BORDER),
            "Menu.background" to BG, "Menu.foreground" to TEXT, "Menu.selectionBackground" to HOVER, "Menu.selectionForeground" to TEXT,
            "Menu.borderPainted" to false, "MenuItem.background" to PANEL, "MenuItem.foreground" to TEXT,
            "MenuItem.selectionBackground" to SELECTED, "MenuItem.selectionForeground" to TEXT, "MenuItem.acceleratorForeground" to TEXT_DIM,
            "MenuItem.acceleratorSelectionForeground" to TEXT_DIM, "MenuItem.borderPainted" to false,
            "PopupMenu.background" to PANEL, "PopupMenu.border" to BorderFactory.createLineBorder(BORDER),
            "Separator.foreground" to BORDER, "Separator.background" to PANEL, "PopupMenuSeparator.foreground" to BORDER,
            "TabbedPane.background" to BG, "TabbedPane.foreground" to TEXT, "TabbedPane.selected" to PANEL, "TabbedPane.contentAreaColor" to PANEL,
            "TabbedPane.borderHightlightColor" to BORDER, "TabbedPane.darkShadow" to BG, "TabbedPane.shadow" to BG, "TabbedPane.light" to BORDER,
            "TabbedPane.highlight" to BORDER, "TabbedPane.focus" to PANEL, "TabbedPane.unselectedBackground" to BG, "TabbedPane.tabAreaBackground" to BG,
            "TabbedPane.contentBorderInsets" to Insets(0, 0, 0, 0), "TabbedPane.tabInsets" to Insets(6, 16, 6, 16), "TabbedPane.selectHighlight" to ACCENT,
            "TextField.background" to BG, "TextField.foreground" to TEXT, "TextField.caretForeground" to TEXT, "TextField.selectionBackground" to SELECTED,
            "TextField.border" to BorderFactory.createCompoundBorder(BorderFactory.createLineBorder(BORDER), BorderFactory.createEmptyBorder(3, 6, 3, 6)),
            "FormattedTextField.background" to BG, "FormattedTextField.foreground" to TEXT, "FormattedTextField.caretForeground" to TEXT,
            "Spinner.background" to BG, "Spinner.border" to BorderFactory.createLineBorder(BORDER), "Spinner.arrowButtonBorder" to BorderFactory.createEmptyBorder(),
            "ComboBox.background" to BG, "ComboBox.foreground" to TEXT, "ComboBox.selectionBackground" to SELECTED, "ComboBox.selectionForeground" to TEXT,
            "ComboBox.buttonBackground" to PANEL_LIGHT, "ComboBox.buttonDarkShadow" to BORDER, "ComboBox.buttonHighlight" to PANEL_LIGHT, "ComboBox.buttonShadow" to BORDER,
            "List.background" to PANEL, "List.foreground" to TEXT, "List.selectionBackground" to SELECTED, "List.selectionForeground" to TEXT,
            "ScrollPane.background" to PANEL, "Viewport.background" to PANEL, "ScrollBar.width" to 10,
            "Slider.background" to PANEL, "Slider.foreground" to ACCENT, "Slider.focus" to PANEL, "Slider.thumb" to ACCENT, "Slider.highlight" to BORDER, "Slider.shadow" to BORDER,
            "CheckBox.background" to PANEL, "CheckBox.foreground" to TEXT, "Button.background" to PANEL_LIGHT, "Button.foreground" to TEXT,
            "Button.select" to PRESSED, "Button.focus" to PANEL_LIGHT, "ToggleButton.background" to PANEL_LIGHT, "ToggleButton.foreground" to TEXT,
            "ToolTip.background" to Color(0x2B, 0x2B, 0x30), "ToolTip.foreground" to TEXT,
            "ToolTip.border" to BorderFactory.createCompoundBorder(BorderFactory.createLineBorder(BORDER), BorderFactory.createEmptyBorder(4, 8, 4, 8)),
            "TextArea.background" to BG, "TextArea.foreground" to TEXT, "SplitPane.background" to BG, "SplitPane.dividerSize" to 4,
            "FileChooser.background" to PANEL, "ColorChooser.background" to PANEL, "ColorChooser.foreground" to TEXT,
            "TitledBorder.titleColor" to TEXT_DIM,
        )
        javax.swing.ToolTipManager.sharedInstance().initialDelay = 450
    }

    // ---- controls ---------------------------------------------------------------------------

    /** Flat button: transparent until hovered, accent-tinted when selected (toggle buttons). */
    private class FlatUI(private val radius: Int = 6) : BasicButtonUI() {
        override fun installDefaults(b: AbstractButton) {
            super.installDefaults(b)
            b.isOpaque = false; b.isContentAreaFilled = false; b.isBorderPainted = false; b.isFocusPainted = false
            b.isRolloverEnabled = true; b.foreground = TEXT
            b.border = BorderFactory.createEmptyBorder(5, 10, 5, 10)
        }

        override fun paint(g0: Graphics, c: JComponent) {
            val b = c as AbstractButton
            val g = g0.create() as Graphics2D
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            val m = b.model
            val bg = when {
                m.isPressed -> PRESSED
                b.isSelected -> SELECTED
                m.isRollover -> HOVER
                b.getClientProperty("filled") == true -> PANEL_LIGHT
                else -> null
            }
            if (bg != null) { g.color = bg; g.fill(RoundRectangle2D.Float(0f, 0f, c.width.toFloat(), c.height.toFloat(), radius * 2f, radius * 2f)) }
            if (b.isSelected) {
                g.color = ACCENT
                if (b.getClientProperty("rail") == true) g.fillRoundRect(0, 6, 3, c.height - 12, 3, 3)
                else g.draw(RoundRectangle2D.Float(0.5f, 0.5f, c.width - 1f, c.height - 1f, radius * 2f, radius * 2f))
            }
            g.dispose()
            super.paint(g0, c)
        }
    }

    fun flatButton(text: String, tip: String? = null, action: () -> Unit) = JButton(text).apply {
        setUI(FlatUI()); putClientProperty("filled", true); toolTipText = tip; isFocusable = false
        foreground = TEXT
        addActionListener { action() }
    }

    /** Kept for the 3D and game panels. */
    fun button(text: String, tip: String? = null, action: () -> Unit) = flatButton(text, tip, action)

    fun iconButton(icon: Icon, tip: String, action: () -> Unit) = JButton(icon).apply {
        setUI(FlatUI(4)); toolTipText = tip; isFocusable = false
        border = BorderFactory.createEmptyBorder(3, 3, 3, 3)
        addActionListener { action() }
    }

    fun iconToggle(icon: Icon, tip: String, action: () -> Unit) = JToggleButton(icon).apply {
        setUI(FlatUI()); putClientProperty("rail", true); isFocusable = false
        toolTipText = "<html>" + tip.replace("\n", "<br><span style='color:#9C9CA6'>") + "</span></html>"
        border = BorderFactory.createEmptyBorder(8, 8, 8, 8)
        alignmentX = Component.CENTER_ALIGNMENT
        maximumSize = Dimension(40, 38)
        addActionListener { action() }
    }

    fun chipToggle(text: String, initial: Boolean, tip: String? = null, onChange: (Boolean) -> Unit) = JToggleButton(text, initial).apply {
        setUI(FlatUI()); putClientProperty("filled", true); toolTipText = tip; isFocusable = false
        border = BorderFactory.createEmptyBorder(4, 10, 4, 10)
        addActionListener { onChange(isSelected) }
    }

    fun segmented(items: List<String>, selected: Int, onSelect: (Int) -> Unit): JPanel {
        val p = JPanel(FlowLayout(FlowLayout.LEFT, 0, 0)).apply { isOpaque = false }
        val group = ButtonGroup()
        items.forEachIndexed { i, s ->
            val b = JToggleButton(s, i == selected).apply {
                setUI(FlatUI(4)); putClientProperty("filled", true); isFocusable = false
                border = BorderFactory.createEmptyBorder(4, 9, 4, 9)
                addActionListener { onSelect(i) }
            }
            group.add(b); p.add(b)
        }
        return p
    }

    fun field(f: JTextField) = f.apply { background = BG; foreground = TEXT; caretColor = TEXT; font = Font(Font.MONOSPACED, Font.PLAIN, 12) }

    fun combo(items: Array<String>) = styleCombo(JComboBox(items))

    fun <T> styleCombo(cb: JComboBox<T>): JComboBox<T> = cb.apply {
        isFocusable = false; background = BG; foreground = TEXT
        border = BorderFactory.createLineBorder(BORDER)
        setUI(object : javax.swing.plaf.basic.BasicComboBoxUI() {
            override fun createArrowButton() = JButton(ChevronIcon(true)).apply {
                isContentAreaFilled = false; isBorderPainted = false; isFocusable = false; border = BorderFactory.createEmptyBorder(0, 6, 0, 8)
            }
            override fun paintCurrentValueBackground(g: Graphics, b: Rectangle, f: Boolean) { g.color = BG; g.fillRect(b.x, b.y, b.width, b.height) }
        })
    }

    /** Flat slider: thin track, accent fill up to a round thumb. */
    private class FlatSliderUI(s: JSlider) : javax.swing.plaf.basic.BasicSliderUI(s) {
        override fun paintTrack(g0: Graphics) {
            val g = g0 as Graphics2D
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            val y = trackRect.y + trackRect.height / 2 - 2
            g.color = BORDER; g.fillRoundRect(trackRect.x, y, trackRect.width, 4, 4, 4)
            g.color = ACCENT; g.fillRoundRect(trackRect.x, y, thumbRect.x + thumbRect.width / 2 - trackRect.x, 4, 4, 4)
        }
        override fun paintThumb(g0: Graphics) {
            val g = g0 as Graphics2D
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            val d = 14; val x = thumbRect.x + (thumbRect.width - d) / 2; val y = thumbRect.y + (thumbRect.height - d) / 2
            g.color = TEXT; g.fillOval(x, y, d, d)
            g.color = ACCENT; g.fillOval(x + 4, y + 4, d - 8, d - 8)
        }
        override fun paintFocus(g: Graphics) {}
        override fun getThumbSize() = Dimension(16, 16)
    }

    /** Rounded check box glyph. */
    private class CheckIcon : Icon {
        override fun getIconWidth() = 16
        override fun getIconHeight() = 16
        override fun paintIcon(c: Component?, g0: Graphics, x: Int, y: Int) {
            val g = g0.create() as Graphics2D
            g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
            val on = (c as? AbstractButton)?.isSelected == true
            g.color = if (on) ACCENT else BG; g.fillRoundRect(x, y, 15, 15, 5, 5)
            g.color = if (on) ACCENT else BORDER; g.drawRoundRect(x, y, 15, 15, 5, 5)
            if (on) { g.color = Color.WHITE; g.stroke = BasicStroke(1.8f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND); g.drawPolyline(intArrayOf(x + 4, x + 7, x + 12), intArrayOf(y + 8, y + 11, y + 5), 3) }
            g.dispose()
        }
    }

    /** Restyles a panel built from stock Swing widgets (the 3D and game tools) to match the theme. */
    fun styleTree(root: Component) {
        when (root) {
            is JComboBox<*> -> styleCombo(root)
            is JCheckBox -> { root.icon = CheckIcon(); root.selectedIcon = CheckIcon(); root.isOpaque = false; root.foreground = TEXT; root.isFocusPainted = false; root.iconTextGap = 6 }
            is JToggleButton, is JButton -> if ((root as AbstractButton).ui !is FlatUI && root.parent !is JComboBox<*> && root.parent !is JScrollBar && root.parent !is JSpinner) {
                root.setUI(FlatUI()); root.putClientProperty("filled", true); root.border = BorderFactory.createEmptyBorder(5, 10, 5, 10)
            }
            is JSlider -> { root.setUI(FlatSliderUI(root)); root.isOpaque = false }
            is JSpinner -> root.preferredSize = Dimension(max(root.preferredSize.width, 64), max(root.preferredSize.height, 26))
            is JScrollPane -> scroll(root)
        }
        if (root is java.awt.Container && root !is JComboBox<*> && root !is JSpinner) root.components.forEach { styleTree(it) }
    }

    fun small(text: String) = JLabel(text).apply { foreground = TEXT_DIM; font = font.deriveFont(12f) }

    fun hint(text: String) = JTextArea(text).apply {
        isEditable = false; lineWrap = true; wrapStyleWord = true; isOpaque = false; foreground = TEXT_DIM; font = FONT.deriveFont(12f)
        border = BorderFactory.createEmptyBorder(2, 0, 6, 0); alignmentX = Component.LEFT_ALIGNMENT
        maximumSize = Dimension(Int.MAX_VALUE, 60)
    }

    fun separator(): JComponent = object : JComponent() {
        init { preferredSize = Dimension(28, 1); maximumSize = Dimension(28, 1); alignmentX = Component.CENTER_ALIGNMENT }
        override fun paintComponent(g: Graphics) { g.color = BORDER; g.fillRect(0, 0, width, 1) }
    }

    fun separatorV(): JComponent = object : JComponent() {
        init { preferredSize = Dimension(9, 22) }
        override fun paintComponent(g: Graphics) { g.color = BORDER; g.fillRect(4, 2, 1, height - 4) }
    }

    fun tabs() = javax.swing.JTabbedPane().apply {
        background = BG; foreground = TEXT; isFocusable = false
        setUI(object : javax.swing.plaf.basic.BasicTabbedPaneUI() {
            override fun installDefaults() { super.installDefaults(); tabAreaInsets = Insets(4, 8, 0, 8); contentBorderInsets = Insets(0, 0, 0, 0) }
            override fun paintTabBackground(g: Graphics, p: Int, i: Int, x: Int, y: Int, w: Int, h: Int, sel: Boolean) {
                g.color = if (sel) PANEL else BG; g.fillRect(x, y, w, h)
            }
            override fun paintTabBorder(g: Graphics, p: Int, i: Int, x: Int, y: Int, w: Int, h: Int, sel: Boolean) {
                if (sel) { g.color = ACCENT; g.fillRect(x + 8, y + h - 2, w - 16, 2) }
            }
            override fun paintContentBorder(g: Graphics, p: Int, s: Int) {}
            override fun paintFocusIndicator(g: Graphics, p: Int, r: Array<Rectangle>, i: Int, ir: Rectangle, tr: Rectangle, sel: Boolean) {}
        })
    }

    fun scroll(sp: JScrollPane) {
        for (bar in listOf(sp.verticalScrollBar, sp.horizontalScrollBar)) {
            bar.setUI(object : BasicScrollBarUI() {
                override fun configureScrollBarColors() { thumbColor = Color(0x4A, 0x4A, 0x52); trackColor = PANEL }
                override fun createDecreaseButton(o: Int) = JButton().apply { preferredSize = Dimension(0, 0) }
                override fun createIncreaseButton(o: Int) = JButton().apply { preferredSize = Dimension(0, 0) }
                override fun paintThumb(g0: Graphics, c: JComponent, r: Rectangle) {
                    val g = g0 as Graphics2D
                    g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
                    g.color = if (isThumbRollover) Color(0x60, 0x60, 0x6A) else thumbColor
                    g.fillRoundRect(r.x + 2, r.y + 2, r.width - 4, r.height - 4, 6, 6)
                }
                override fun paintTrack(g: Graphics, c: JComponent, r: Rectangle) { g.color = c.parent?.background ?: PANEL; g.fillRect(r.x, r.y, r.width, r.height) }
            })
            bar.preferredSize = if (bar.orientation == JScrollBar.VERTICAL) Dimension(10, 0) else Dimension(0, 10)
            bar.unitIncrement = 16
        }
        sp.viewport.background = PANEL
    }

    /** A dock section with a clickable header that folds its content. */
    fun collapsible(title: String, content: JComponent, open: Boolean): JPanel {
        val box = JPanel(BorderLayout()).apply { isOpaque = false; alignmentX = Component.LEFT_ALIGNMENT }
        val chevron = ChevronIcon(open)
        val header = JLabel(title, chevron, SwingConstants.LEFT).apply {
            iconTextGap = 6
            foreground = TEXT_DIM; font = font.deriveFont(Font.BOLD, 11f)
            border = BorderFactory.createEmptyBorder(12, 0, 6, 0)
            cursor = Cursor.getPredefinedCursor(Cursor.HAND_CURSOR)
        }
        content.isVisible = open
        header.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                content.isVisible = !content.isVisible
                chevron.open = content.isVisible
                header.repaint()
                box.revalidate()
            }
        })
        box.add(header, BorderLayout.NORTH)
        box.add(content, BorderLayout.CENTER)
        box.maximumSize = Dimension(Int.MAX_VALUE, Int.MAX_VALUE)
        return box
    }

    fun section(title: String): Border = BorderFactory.createCompoundBorder(
        BorderFactory.createEmptyBorder(6, 6, 6, 6),
        BorderFactory.createTitledBorder(BorderFactory.createLineBorder(BORDER), title, 0, 0, FONT.deriveFont(Font.BOLD, 11f), TEXT_DIM),
    )

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

// ═══ Icons ═══════════════════════════════════════════════════════════════════════════════════════

/** Vector tool icons drawn with Java2D (crisp at every UI scale, no image assets). */
class ToolIcon(private val tool: Tool, private val size: Int = 20) : Icon {
    override fun getIconWidth() = size
    override fun getIconHeight() = size

    override fun paintIcon(c: Component?, g0: Graphics, x: Int, y: Int) {
        val g = g0.create() as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
        g.translate(x, y)
        g.scale(size / 20.0, size / 20.0)
        g.stroke = BasicStroke(1.6f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND)
        val selected = (c as? AbstractButton)?.isSelected == true
        g.color = if (selected) Color.WHITE else Theme.TEXT
        when (tool) {
            Tool.PENCIL -> {
                val p = Path2D.Float().apply { moveTo(4.0, 16.0); lineTo(5.0, 12.5); lineTo(13.5, 4.0); lineTo(16.0, 6.5); lineTo(7.5, 15.0); closePath() }
                g.draw(p); g.drawLine(11, 6, 14, 9)
            }
            Tool.ERASER -> {
                val p = Path2D.Float().apply { moveTo(3.5, 12.5); lineTo(10.5, 5.5); lineTo(16.5, 11.5); lineTo(11.5, 16.5); lineTo(7.5, 16.5); closePath() }
                g.draw(p); g.drawLine(7, 9, 13, 15); g.drawLine(8, 17, 17, 17)
            }
            Tool.SHADE -> {
                g.drawOval(4, 4, 12, 12)
                g.fill(java.awt.geom.Arc2D.Float(4f, 4f, 12f, 12f, 270f, 180f, java.awt.geom.Arc2D.CHORD))
                g.color = Theme.ACCENT; g.fillRect(9, 2, 2, 2)
            }
            Tool.FILL -> {
                val p = Path2D.Float().apply { moveTo(4.0, 9.0); lineTo(9.0, 4.0); lineTo(15.0, 10.0); lineTo(10.0, 15.0); closePath() }
                g.draw(p); g.drawLine(6, 7, 13, 7)
                g.fill(java.awt.geom.Ellipse2D.Float(14.5f, 12.5f, 3f, 4f))
            }
            Tool.PICKER -> {
                g.drawLine(4, 16, 11, 9)
                g.drawLine(9, 7, 13, 11)
                g.fillRoundRect(11, 3, 6, 6, 3, 3)
                g.drawLine(3, 17, 5, 15)
            }
            Tool.LINE -> { g.drawLine(4, 16, 16, 4); g.fillOval(2, 14, 4, 4); g.fillOval(14, 2, 4, 4) }
            Tool.RECTANGLE -> g.drawRoundRect(3, 5, 14, 10, 2, 2)
            Tool.ELLIPSE -> g.drawOval(3, 4, 14, 12)
            Tool.SELECT -> {
                g.stroke = BasicStroke(1.5f, BasicStroke.CAP_BUTT, BasicStroke.JOIN_MITER, 10f, floatArrayOf(2.5f, 2f), 0f)
                g.drawRect(3, 4, 14, 12)
            }
            Tool.MOVE -> {
                g.drawLine(10, 3, 10, 17); g.drawLine(3, 10, 17, 10)
                g.drawPolyline(intArrayOf(8, 10, 12), intArrayOf(5, 3, 5), 3); g.drawPolyline(intArrayOf(8, 10, 12), intArrayOf(15, 17, 15), 3)
                g.drawPolyline(intArrayOf(5, 3, 5), intArrayOf(8, 10, 12), 3); g.drawPolyline(intArrayOf(15, 17, 15), intArrayOf(8, 10, 12), 3)
            }
            Tool.HAND -> {
                val p = Path2D.Float().apply {
                    moveTo(6.0, 11.0); lineTo(6.0, 6.0); quadTo(7.0, 4.5, 8.0, 6.0); lineTo(8.0, 10.0); lineTo(8.0, 4.5); quadTo(9.0, 3.0, 10.0, 4.5)
                    lineTo(10.0, 10.0); lineTo(10.0, 5.0); quadTo(11.0, 3.5, 12.0, 5.0); lineTo(12.0, 10.0); lineTo(12.0, 7.0); quadTo(13.0, 5.5, 14.0, 7.0)
                    lineTo(14.0, 13.0); quadTo(13.5, 17.0, 10.0, 17.0); lineTo(9.0, 17.0); quadTo(6.5, 17.0, 5.0, 14.0); lineTo(3.5, 11.0); quadTo(4.5, 9.5, 6.0, 11.0)
                }
                g.draw(p)
            }
        }
        g.dispose()
    }
}

/** Fold indicator of dock sections. */
class ChevronIcon(var open: Boolean) : Icon {
    override fun getIconWidth() = 10
    override fun getIconHeight() = 10
    override fun paintIcon(c: Component?, g0: Graphics, x: Int, y: Int) {
        val g = g0.create() as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
        g.color = Theme.TEXT_DIM
        g.stroke = BasicStroke(1.5f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND)
        if (open) g.drawPolyline(intArrayOf(x + 1, x + 5, x + 9), intArrayOf(y + 3, y + 7, y + 3), 3)
        else g.drawPolyline(intArrayOf(x + 3, x + 7, x + 3), intArrayOf(y + 1, y + 5, y + 9), 3)
        g.dispose()
    }
}

/** Small glyphs for layer rows (visibility and lock). */
class GlyphIcon(private val kind: Kind) : Icon {
    enum class Kind { EYE, EYE_OFF, LOCK, UNLOCK }

    override fun getIconWidth() = 16
    override fun getIconHeight() = 16

    override fun paintIcon(c: Component?, g0: Graphics, x: Int, y: Int) {
        val g = g0.create() as Graphics2D
        g.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
        g.translate(x, y)
        g.stroke = BasicStroke(1.4f, BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND)
        when (kind) {
            Kind.EYE, Kind.EYE_OFF -> {
                g.color = if (kind == Kind.EYE) Theme.TEXT else Theme.TEXT_DIM
                val p = Path2D.Float().apply { moveTo(1.5, 8.0); quadTo(8.0, 1.5, 14.5, 8.0); quadTo(8.0, 14.5, 1.5, 8.0) }
                g.draw(p)
                if (kind == Kind.EYE) g.fillOval(6, 6, 4, 4) else g.drawLine(3, 13, 13, 3)
            }
            Kind.LOCK, Kind.UNLOCK -> {
                g.color = if (kind == Kind.LOCK) Theme.WARN else Theme.TEXT_DIM
                g.drawRoundRect(3, 7, 10, 7, 2, 2)
                if (kind == Kind.LOCK) g.drawArc(5, 2, 6, 9, 0, 180) else g.drawArc(5, 2, 6, 9, 20, 160)
                if (kind == Kind.LOCK) { g.drawLine(5, 6, 5, 7); g.drawLine(11, 6, 11, 7) } else g.drawLine(5, 6, 5, 7)
            }
        }
        g.dispose()
    }
}
