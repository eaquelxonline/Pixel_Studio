package com.pixel.studio.desktop

import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.AgentGateway
import com.pixel.studio.json.list
import com.pixel.studio.gateway.GatewayConfig
import com.pixel.studio.gateway.tools.Confirmer
import com.pixel.studio.gateway.tools.DirectoryWorkspace
import java.awt.BorderLayout
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.GridLayout
import java.awt.Image
import java.awt.event.KeyEvent
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.awt.image.BufferedImage
import java.io.File
import java.net.BindException
import javax.imageio.ImageIO
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.ButtonGroup
import javax.swing.JFileChooser
import javax.swing.JFrame
import javax.swing.JLabel
import javax.swing.JMenu
import javax.swing.JMenuBar
import javax.swing.JMenuItem
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JSpinner
import javax.swing.JToggleButton
import javax.swing.KeyStroke
import javax.swing.SpinnerNumberModel
import javax.swing.SwingUtilities
import javax.swing.filechooser.FileNameExtensionFilter

const val APP_ID = "com.pixel.studio"

class PixelStudioWindow(private val c: EditorController, private val gateway: AgentGateway) : JFrame("Pixel Studio") {
    private val canvas = CanvasView(c)
    val voxels = VoxelPanel(c)
    private val status = JLabel(" ")
    private var cursorText = ""

    init {
        defaultCloseOperation = DO_NOTHING_ON_CLOSE
        iconImages = listOf("icon-32.png", "icon-64.png", "icon-256.png").mapNotNull { name ->
            javaClass.getResource(name)?.let { ImageIO.read(it) as Image }
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
            add(item("Flip horizontal") { c.run("flip_horizontal") })
            add(item("Flip vertical") { c.run("flip_vertical") })
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
            val names = c.engine.execute("palette_list").result.list("palettes").orEmpty().map { (it as Map<*, *>)["name"] as String }
            add(JMenu("Load palette").apply { names.forEach { n -> add(item(n) { c.run("palette_load", mapOf("name" to n)) }) } })
            add(item("Quantize to swatches (bayer4 dither)") { c.run("quantize", mapOf("scope" to "frame", "palette" to "swatches", "dither" to "bayer4")) })
            addSeparator()
            add(item("Export normal map…") {
                val ch = chooser("png", "PNG")
                ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + "_normal.png")
                if (ch.showSaveDialog(this@PixelStudioWindow) == JFileChooser.APPROVE_OPTION) {
                    ch.selectedFile.writeBytes(c.run("export_normal_map", mapOf("frame" to c.doc.activeFrame)).binary)
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
        val (op, ext) = when (format) {
            "gif" -> "export_gif" to "gif"
            "spritesheet" -> "export_spritesheet" to "png"
            else -> "export_png" to "png"
        }
        val scale = JOptionPane.showInputDialog(this, "Scale (1-64)", "1")?.toIntOrNull()?.coerceIn(1, 64) ?: return
        val ch = chooser(ext, ext.uppercase())
        ch.selectedFile = File(DirectoryWorkspace.sanitize(c.doc.name) + (if (format == "spritesheet") "_sheet." else ".") + ext)
        if (ch.showSaveDialog(this) != JFileChooser.APPROVE_OPTION) return
        val r = c.run(op, mapOf("scale" to scale, "frame" to c.doc.activeFrame))
        if (!r.ok) return
        ch.selectedFile.writeBytes(r.binary)
        if (format == "spritesheet") File(ch.selectedFile.path.removeSuffix(".png") + ".json").writeText(com.pixel.studio.json.Json.write(r.result))
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
