// Pixel Studio — Android editor (platform Views only): 2D canvas, 3D viewer and Agent Mode service.

package com.pixel.studio.android

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.Button
import android.widget.EditText
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import com.pixel.studio.R
import com.pixel.studio.gateway.AgentGateway
import com.pixel.studio.gateway.Confirmer
import com.pixel.studio.gateway.DirectoryWorkspace
import com.pixel.studio.gateway.GatewayConfig
import com.pixel.studio.gateway.PixelEngine
import com.pixel.studio.gateway.list
import com.pixel.studio.gateway.long
import com.pixel.studio.gateway.str
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

// ═══ PixelStudioApplication ══════════════════════════════════════════════════════════════════════

/**
 * Process-wide state: one engine shared by the editor UI and the Agent Mode service, so a
 * human and an AI always work on the same canvas.
 */
class PixelStudioApplication : Application() {
    lateinit var engine: PixelEngine
        private set
    var gateway: AgentGateway? = null
    var foreground: WeakReference<Activity> = WeakReference(null)

    val workspaceDir: File get() = File(filesDir, "workspace").apply { mkdirs() }
    val autosaveFile: File get() = File(filesDir, "autosave.pxs")

    override fun onCreate() {
        super.onCreate()
        engine = PixelEngine(32, 32)
        if (autosaveFile.isFile) {
            runCatching { engine.execute("project_load", payload = autosaveFile.readBytes()) }
        }
    }

    fun autosave() {
        val r = engine.execute("project_save")
        if (!r.ok) return
        val tmp = File(filesDir, "autosave.tmp")
        tmp.writeBytes(r.binary)
        tmp.renameTo(autosaveFile)
    }

    fun startGateway(): AgentGateway {
        gateway?.let { if (it.running) return it }
        // Loopback only: reachable by agents on this device, or from a computer via
        // `adb forward tcp:38471 tcp:38471` (USB, never exposed to the network).
        val g = AgentGateway(engine, GatewayConfig(), DirectoryWorkspace(workspaceDir), confirmer).start()
        gateway = g
        return g
    }

    fun stopGateway() {
        gateway?.close()
        gateway = null
    }

    /** Destructive agent actions show a dialog on the visible activity; no activity = denied. */
    private val confirmer = Confirmer { actor, _, summary ->
        val activity = foreground.get() ?: return@Confirmer false
        val latch = CountDownLatch(1)
        var allowed = false
        Handler(Looper.getMainLooper()).post {
            AlertDialog.Builder(activity)
                .setTitle("Agent permission")
                .setMessage("$actor wants to run a destructive action:\n\n$summary")
                .setPositiveButton("Allow") { _, _ -> allowed = true; latch.countDown() }
                .setNegativeButton("Deny") { _, _ -> latch.countDown() }
                .setOnCancelListener { latch.countDown() }
                .show()
        }
        latch.await(60, TimeUnit.SECONDS)
        allowed
    }
}

// ═══ UI kit ══════════════════════════════════════════════════════════════════════════════════════

/** Neutral dark palette shared with the desktop editor: greys keep pixel colors true, one accent. */
object Ui {
    const val BG = 0xFF1B1B1E.toInt()
    const val PANEL = 0xFF232327.toInt()
    const val RAISED = 0xFF2D2D32.toInt()
    const val SELECTED = 0xFF2C364D.toInt()
    const val BORDER = 0xFF37373D.toInt()
    const val CANVAS = 0xFF161619.toInt()
    const val TEXT = 0xFFECECEF.toInt()
    const val DIM = 0xFF9C9CA6.toInt()
    const val ACCENT = 0xFF5B9BFF.toInt()
    const val OK = 0xFF5CD18C.toInt()
    const val WARN = 0xFFFFB057.toInt()
    const val CHECK_A = 0xFF3A3A40.toInt()
    const val CHECK_B = 0xFF303035.toInt()

    fun parse(hex: String): Int = runCatching {
        val h = hex.removePrefix("#")
        if (h.length == 8) Color.argb(h.substring(6, 8).toInt(16), h.substring(0, 2).toInt(16), h.substring(2, 4).toInt(16), h.substring(4, 6).toInt(16))
        else Color.parseColor("#" + h.take(6))
    }.getOrDefault(Color.MAGENTA)

    fun hex(c: Int) = String.format("#%02X%02X%02X", Color.red(c), Color.green(c), Color.blue(c))

    fun round(color: Int, radius: Float, stroke: Int = 0, strokeColor: Int = BORDER) = GradientDrawable().apply {
        cornerRadius = radius; setColor(color); if (stroke > 0) setStroke(stroke, strokeColor)
    }
}

enum class Tool(val label: String, val hint: String) {
    PENCIL("Pencil", "Pixel-perfect freehand"),
    ERASER("Eraser", "Erase to transparent"),
    SHADE("Shade", "Steps colors along their ramp · tap Darker/Lighter"),
    FILL("Fill", "Bucket fill"),
    PICKER("Picker", "Pick a color"),
    LINE("Line", "Straight line"),
    RECTANGLE("Rect", "Rectangle"),
    ELLIPSE("Ellipse", "Ellipse"),
    MOVE("Move", "Drag to move the layer"),
}

/** Vector tool glyph (same drawings as the desktop rail), sized in dp. */
class ToolGlyph(private val tool: Tool, private val density: Float) : android.graphics.drawable.Drawable() {
    var glyphColor = Ui.TEXT
    private val p = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; strokeWidth = 1.6f; strokeCap = Paint.Cap.ROUND; strokeJoin = Paint.Join.ROUND }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)

    override fun draw(c: Canvas) {
        val b = bounds
        val s = min(b.width(), b.height()) / 20f
        c.save()
        c.translate(b.left + (b.width() - 20 * s) / 2, b.top + (b.height() - 20 * s) / 2)
        c.scale(s, s)
        p.color = glyphColor; fill.color = glyphColor
        val path = android.graphics.Path()
        when (tool) {
            Tool.PENCIL -> { path.moveTo(4f, 16f); path.lineTo(5f, 12.5f); path.lineTo(13.5f, 4f); path.lineTo(16f, 6.5f); path.lineTo(7.5f, 15f); path.close(); c.drawPath(path, p); c.drawLine(11f, 6f, 14f, 9f, p) }
            Tool.ERASER -> { path.moveTo(3.5f, 12.5f); path.lineTo(10.5f, 5.5f); path.lineTo(16.5f, 11.5f); path.lineTo(11.5f, 16.5f); path.lineTo(7.5f, 16.5f); path.close(); c.drawPath(path, p); c.drawLine(7f, 9f, 13f, 15f, p) }
            Tool.SHADE -> { c.drawCircle(10f, 10f, 6f, p); c.drawArc(4f, 4f, 16f, 16f, -90f, 180f, true, fill) }
            Tool.FILL -> { path.moveTo(4f, 9f); path.lineTo(9f, 4f); path.lineTo(15f, 10f); path.lineTo(10f, 15f); path.close(); c.drawPath(path, p); c.drawOval(14.5f, 12.5f, 17.5f, 16.5f, fill) }
            Tool.PICKER -> { c.drawLine(4f, 16f, 11f, 9f, p); c.drawLine(9f, 7f, 13f, 11f, p); c.drawRoundRect(11f, 3f, 17f, 9f, 3f, 3f, fill) }
            Tool.LINE -> { c.drawLine(4f, 16f, 16f, 4f, p); c.drawCircle(4f, 16f, 2f, fill); c.drawCircle(16f, 4f, 2f, fill) }
            Tool.RECTANGLE -> c.drawRoundRect(3f, 5f, 17f, 15f, 1.5f, 1.5f, p)
            Tool.ELLIPSE -> c.drawOval(3f, 4f, 17f, 16f, p)
            Tool.MOVE -> {
                c.drawLine(10f, 3f, 10f, 17f, p); c.drawLine(3f, 10f, 17f, 10f, p)
                for ((a, bb) in listOf(Pair(floatArrayOf(8f, 10f, 12f), floatArrayOf(5f, 3f, 5f)), Pair(floatArrayOf(8f, 10f, 12f), floatArrayOf(15f, 17f, 15f)),
                    Pair(floatArrayOf(5f, 3f, 5f), floatArrayOf(8f, 10f, 12f)), Pair(floatArrayOf(15f, 17f, 15f), floatArrayOf(8f, 10f, 12f)))) {
                    c.drawLine(a[0], bb[0], a[1], bb[1], p); c.drawLine(a[1], bb[1], a[2], bb[2], p)
                }
            }
        }
        c.restore()
    }

    override fun setAlpha(alpha: Int) {}
    override fun setColorFilter(cf: android.graphics.ColorFilter?) {}
    @Deprecated("Deprecated in platform API") override fun getOpacity() = android.graphics.PixelFormat.TRANSLUCENT
}

// ═══ MainActivity ════════════════════════════════════════════════════════════════════════════════

/** Editor screen built from platform Views only (no AndroidX, no Compose). */
class MainActivity : Activity() {
    private companion object {
        const val REQ_SAVE = 1
        const val REQ_OPEN = 2
        const val REQ_EXPORT_PNG = 3
        const val REQ_EXPORT_GIF = 4
        const val REQ_IMPORT = 5
        const val REQ_REFERENCE = 6
        const val REQ_CONCEPT = 7
    }

    private val app get() = application as PixelStudioApplication
    private val engine get() = app.engine
    private lateinit var canvas: PixelCanvasView
    private lateinit var voxels: VoxelView
    private lateinit var title: TextView
    private lateinit var options: LinearLayout
    private lateinit var tools: LinearLayout
    private lateinit var colorChip: View
    private lateinit var frames: LinearLayout
    private lateinit var status: TextView
    private lateinit var layersPanel: LinearLayout
    private lateinit var layerRows: LinearLayout
    private val toolViews = LinkedHashMap<Tool, android.widget.ImageView>()
    private val main = Handler(Looper.getMainLooper())
    private var refreshPosted = false
    private var playing: Runnable? = null
    private var info: Map<String, Any?> = emptyMap()
    private val listener: (Long) -> Unit = { scheduleRefresh() }

    private fun dp(v: Float) = (v * resources.displayMetrics.density).toInt()
    private fun dp(v: Int) = dp(v.toFloat())

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = Ui.BG
        window.navigationBarColor = Ui.PANEL
        canvas = PixelCanvasView(this, engine).apply { onColorPicked = { pickColor(it) } }
        voxels = VoxelView(this, engine).apply { visibility = View.GONE }
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(Ui.BG) }
        root.addView(appBar())
        options = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(8), dp(4), dp(8), dp(4)) }
        root.addView(HorizontalScrollView(this).apply { isHorizontalScrollBarEnabled = false; setBackgroundColor(Ui.PANEL); addView(options) })
        root.addView(divider())
        layersPanel = buildLayersPanel()
        val stage = android.widget.FrameLayout(this).apply {
            addView(canvas); addView(voxels)
            addView(layersPanel, android.widget.FrameLayout.LayoutParams(dp(280), MATCH_PARENT, Gravity.END))
        }
        root.addView(stage, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        root.addView(divider())
        frames = LinearLayout(this).apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(8), dp(6), dp(8), dp(6)) }
        root.addView(HorizontalScrollView(this).apply { isHorizontalScrollBarEnabled = false; setBackgroundColor(Ui.PANEL); addView(frames) })
        root.addView(divider())
        root.addView(toolDock())
        status = TextView(this).apply { setTextColor(Ui.DIM); textSize = 11f; setPadding(dp(14), dp(2), dp(14), dp(6)); setBackgroundColor(Ui.PANEL) }
        root.addView(status)
        setContentView(root)
        engine.addChangeListener(listener)
        selectTool(Tool.PENCIL)
        refreshAll()
    }

    override fun onResume() {
        super.onResume()
        app.foreground = WeakReference(this)
        refreshAll()
    }

    override fun onPause() {
        app.autosave()
        super.onPause()
    }

    override fun onDestroy() {
        engine.removeChangeListener(listener)
        super.onDestroy()
    }

    private fun scheduleRefresh() {
        synchronized(this) { if (refreshPosted) return; refreshPosted = true }
        main.post {
            synchronized(this) { refreshPosted = false }
            refreshAll()
        }
    }

    private fun layers(): List<Map<*, *>> = info.list("layers").orEmpty().map { it as Map<*, *> }
    private fun activeLayer() = info.long("activeLayer")?.toInt() ?: 0
    private fun activeFrame() = info.long("activeFrame")?.toInt() ?: 0

    private fun refreshAll() {
        info = engine.execute("canvas_inspect").result
        val layer = layers().getOrNull(activeLayer())
        canvas.density = (layer?.get("density") as? Long)?.toInt() ?: 1
        canvas.refresh()
        title.text = info.str("name") ?: "Untitled"
        val agent = if (app.gateway?.running == true) "● Agent Mode" else "Agent Mode off"
        val dens = if (canvas.density > 1) " · ${canvas.density}× pixels" else ""
        status.text = "${info.long("width")} × ${info.long("height")} · ${layer?.get("name") ?: ""}$dens · rev ${info.long("revision")} · $agent"
        status.setTextColor(if (app.gateway?.running == true) Ui.OK else Ui.DIM)
        rebuildFrames()
        if (layersPanel.visibility == View.VISIBLE) rebuildLayers()
        colorChip.background = Ui.round(Ui.parse(canvas.color), dp(8f).toFloat(), dp(2), Ui.TEXT)
    }

    // ---- building blocks ----------------------------------------------------------------

    private fun divider() = View(this).apply { setBackgroundColor(Ui.BORDER); layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, 1) }

    private fun chip(text: String, selected: Boolean = false, onClick: () -> Unit) = TextView(this).apply {
        this.text = text
        textSize = 13f
        gravity = Gravity.CENTER
        setTextColor(if (selected) Color.WHITE else Ui.TEXT)
        background = Ui.round(if (selected) Ui.SELECTED else Ui.RAISED, dp(6f).toFloat(), if (selected) dp(1) else 0, Ui.ACCENT)
        setPadding(dp(12), dp(7), dp(12), dp(7))
        layoutParams = LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT).apply { setMargins(dp(3), 0, dp(3), 0) }
        setOnClickListener { onClick() }
    }

    private fun label(text: String) = TextView(this).apply {
        this.text = text; textSize = 12f; setTextColor(Ui.DIM); setPadding(dp(8), 0, dp(4), 0)
    }

    private fun iconButton(text: String, onClick: () -> Unit) = TextView(this).apply {
        this.text = text
        textSize = 18f
        gravity = Gravity.CENTER
        setTextColor(Ui.TEXT)
        background = Ui.round(Color.TRANSPARENT, dp(8f).toFloat())
        layoutParams = LinearLayout.LayoutParams(dp(44), dp(44))
        setOnClickListener { onClick() }
    }

    private fun appBar() = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(12), dp(6), dp(4), dp(6))
        setBackgroundColor(Ui.BG)
        addView(LinearLayout(this@MainActivity).apply {
            orientation = LinearLayout.VERTICAL
            addView(TextView(this@MainActivity).apply { text = "Pixel Studio"; setTextColor(Ui.DIM); textSize = 11f })
            title = TextView(this@MainActivity).apply { setTextColor(Ui.TEXT); textSize = 16f; typeface = Typeface.DEFAULT_BOLD; maxLines = 1 }
            addView(title)
        }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        addView(iconButton("↶") { engine.execute("history_undo") })
        addView(iconButton("↷") { engine.execute("history_redo") })
        addView(chip("Layers") { toggleLayers() })
        addView(chip("3D") { toggle3d() })
        addView(chip("Agent") { agentDialog() })
        addView(iconButton("⋮") {}.apply { setOnClickListener { v -> overflow(v) } })
    }

    private fun toolDock() = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        setBackgroundColor(Ui.PANEL)
        setPadding(dp(6), dp(6), dp(6), dp(4))
        tools = LinearLayout(this@MainActivity).apply { gravity = Gravity.CENTER_VERTICAL }
        for (t in Tool.entries) {
            val v = android.widget.ImageView(this@MainActivity).apply {
                setImageDrawable(ToolGlyph(t, resources.displayMetrics.density))
                setPadding(dp(11), dp(11), dp(11), dp(11))
                contentDescription = t.label
                layoutParams = LinearLayout.LayoutParams(dp(46), dp(46)).apply { setMargins(dp(1), 0, dp(1), 0) }
                setOnClickListener { selectTool(t) }
                setOnLongClickListener { toast("${t.label}: ${t.hint}"); true }
            }
            toolViews[t] = v
            tools.addView(v)
        }
        addView(HorizontalScrollView(this@MainActivity).apply { isHorizontalScrollBarEnabled = false; addView(tools) }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        colorChip = View(this@MainActivity).apply {
            layoutParams = LinearLayout.LayoutParams(dp(40), dp(40)).apply { setMargins(dp(6), 0, dp(6), 0) }
            contentDescription = "Color"
            setOnClickListener { colorSheet() }
        }
        addView(colorChip)
    }

    private fun selectTool(t: Tool) {
        canvas.tool = t
        toolViews.forEach { (tool, v) ->
            val sel = tool == t
            v.background = if (sel) Ui.round(Ui.SELECTED, dp(10f).toFloat(), dp(1), Ui.ACCENT) else null
            (v.drawable as ToolGlyph).glyphColor = if (sel) Color.WHITE else Ui.DIM
            v.invalidate()
        }
        rebuildOptions()
    }

    private fun rebuildOptions() {
        options.removeAllViews()
        options.addView(TextView(this).apply { text = canvas.tool.label; setTextColor(Ui.TEXT); typeface = Typeface.DEFAULT_BOLD; textSize = 13f; setPadding(dp(6), 0, dp(6), 0) })
        val t = canvas.tool
        if (t == Tool.PENCIL || t == Tool.ERASER || t == Tool.SHADE) {
            options.addView(label("Size"))
            for (s in 1..4) options.addView(chip("$s", canvas.brushSize == s) { canvas.brushSize = s; rebuildOptions() })
        }
        if (t == Tool.PENCIL) options.addView(chip("Pixel perfect", canvas.pixelPerfect) { canvas.pixelPerfect = !canvas.pixelPerfect; rebuildOptions() })
        if (t == Tool.SHADE) {
            options.addView(chip("Darker", canvas.shadeSteps < 0) { canvas.shadeSteps = -1; rebuildOptions() })
            options.addView(chip("Lighter", canvas.shadeSteps > 0) { canvas.shadeSteps = 1; rebuildOptions() })
        }
        if (t == Tool.RECTANGLE || t == Tool.ELLIPSE) options.addView(chip("Filled", canvas.filled) { canvas.filled = !canvas.filled; rebuildOptions() })
        if (t == Tool.FILL) options.addView(chip("Contiguous", canvas.contiguous) { canvas.contiguous = !canvas.contiguous; rebuildOptions() })
        if (t in setOf(Tool.PENCIL, Tool.ERASER, Tool.SHADE, Tool.LINE, Tool.RECTANGLE, Tool.ELLIPSE)) {
            options.addView(label("Mirror"))
            options.addView(chip("X", canvas.mirrorX) { canvas.mirrorX = !canvas.mirrorX; canvas.invalidate(); rebuildOptions() })
            options.addView(chip("Y", canvas.mirrorY) { canvas.mirrorY = !canvas.mirrorY; canvas.invalidate(); rebuildOptions() })
        }
        options.addView(label("View"))
        options.addView(chip("Grid", canvas.grid) { canvas.grid = !canvas.grid; canvas.invalidate(); rebuildOptions() })
        options.addView(chip("Onion", canvas.onion) { canvas.onion = !canvas.onion; canvas.refresh(); rebuildOptions() })
        options.addView(chip("Fit") { canvas.fit() })
    }

    private fun pickColor(hex: String) {
        canvas.color = hex
        if (canvas.tool == Tool.PICKER || canvas.tool == Tool.ERASER) selectTool(Tool.PENCIL)
        colorChip.background = Ui.round(Ui.parse(hex), dp(8f).toFloat(), dp(2), Ui.TEXT)
    }

    // ---- frames -------------------------------------------------------------------------

    private fun rebuildFrames() {
        frames.removeAllViews()
        frames.addView(chip(if (playing != null) "■ Stop" else "▶ Play") { togglePlay() })
        val list = info.list("frames").orEmpty().map { it as Map<*, *> }
        list.forEachIndexed { i, f ->
            val active = i == activeFrame()
            frames.addView(TextView(this).apply {
                text = "${i + 1}\n${f["duration"]}ms"
                textSize = 10f
                gravity = Gravity.CENTER
                setTextColor(if (active) Color.WHITE else Ui.DIM)
                background = Ui.round(if (active) Ui.SELECTED else Ui.RAISED, dp(6f).toFloat(), if (active) dp(1) else 0, Ui.ACCENT)
                layoutParams = LinearLayout.LayoutParams(dp(48), dp(40)).apply { setMargins(dp(3), 0, dp(3), 0) }
                setOnClickListener { engine.execute("frame_select", mapOf("frame" to i)) }
                setOnLongClickListener { frameActions(i, (f["duration"] as? Long)?.toInt() ?: 100); true }
            })
        }
        frames.addView(chip("+") { engine.execute("frame_add", mapOf("index" to activeFrame() + 1)) })
        frames.addView(chip("Duplicate") { engine.execute("frame_duplicate", mapOf("frame" to activeFrame())) })
        frames.addView(label("${list.size} frames · ${info.long("totalDurationMs")} ms"))
    }

    private fun frameActions(i: Int, duration: Int) {
        val input = EditText(this).apply { setText("$duration"); inputType = InputType.TYPE_CLASS_NUMBER }
        AlertDialog.Builder(this).setTitle("Frame ${i + 1}").setMessage("Duration (ms)").setView(input)
            .setPositiveButton("Set") { _, _ -> input.text.toString().toIntOrNull()?.let { engine.execute("frame_set_duration", mapOf("frame" to i, "duration" to it)) } }
            .setNeutralButton("Delete") { _, _ -> engine.execute("frame_delete", mapOf("frame" to i)).let { if (!it.ok) toast(it.errorMessage ?: "") } }
            .setNegativeButton("Cancel", null).show()
    }

    private fun togglePlay() {
        playing?.let { main.removeCallbacks(it); playing = null; rebuildFrames(); return }
        val r = object : Runnable {
            override fun run() {
                val list = info.list("frames").orEmpty()
                if (list.isEmpty()) return
                val next = (activeFrame() + 1) % list.size
                engine.execute("frame_select", mapOf("frame" to next))
                main.postDelayed(this, ((list[next] as Map<*, *>)["duration"] as? Long) ?: 100L)
            }
        }
        playing = r
        main.post(r)
        rebuildFrames()
    }

    // ---- layers panel -------------------------------------------------------------------

    private fun buildLayersPanel(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        visibility = View.GONE
        background = Ui.round(Ui.PANEL, 0f, 1, Ui.BORDER)
        elevation = dp(8f).toFloat()
        setPadding(dp(10), dp(10), dp(10), dp(10))
        addView(TextView(this@MainActivity).apply { text = "LAYERS"; textSize = 11f; setTextColor(Ui.DIM); typeface = Typeface.DEFAULT_BOLD; setPadding(dp(4), 0, 0, dp(8)) })
        layerRows = LinearLayout(this@MainActivity).apply { orientation = LinearLayout.VERTICAL }
        addView(ScrollView(this@MainActivity).apply { addView(layerRows) }, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        addView(LinearLayout(this@MainActivity).apply {
            setPadding(0, dp(8), 0, 0)
            addView(chip("+ Layer") { engine.execute("layer_create") })
            addView(chip("▲") { val i = activeLayer(); if (i < layers().size - 1) engine.execute("layer_move", mapOf("layer" to i, "index" to i + 1)) })
            addView(chip("▼") { val i = activeLayer(); if (i > 0) engine.execute("layer_move", mapOf("layer" to i, "index" to i - 1)) })
            addView(chip("⋯") { layers().getOrNull(activeLayer())?.let { layerProperties(it) } })
        })
    }

    private fun toggleLayers() {
        layersPanel.visibility = if (layersPanel.visibility == View.VISIBLE) View.GONE else View.VISIBLE
        if (layersPanel.visibility == View.VISIBLE) rebuildLayers()
    }

    private fun rebuildLayers() {
        layerRows.removeAllViews()
        val frame = activeFrame()
        for (l in layers().reversed()) {
            val index = (l["index"] as Long).toInt()
            val active = index == activeLayer()
            val visible = l["visible"] == true; val locked = l["locked"] == true
            val row = LinearLayout(this).apply {
                gravity = Gravity.CENTER_VERTICAL
                background = Ui.round(if (active) Ui.SELECTED else Color.TRANSPARENT, dp(6f).toFloat())
                setPadding(dp(4), dp(4), dp(8), dp(4))
                setOnClickListener { engine.execute("layer_select", mapOf("layer" to index)) }
                setOnLongClickListener { layerProperties(l); true }
            }
            row.addView(iconButton(if (visible) "◉" else "○") { engine.execute("layer_set", mapOf("layer" to index, "visible" to !visible)) }.apply {
                textSize = 15f; setTextColor(if (visible) Ui.TEXT else Ui.DIM); layoutParams = LinearLayout.LayoutParams(dp(34), dp(34))
            })
            row.addView(iconButton(if (locked) "🔒" else "·") { engine.execute("layer_set", mapOf("layer" to index, "locked" to !locked)) }.apply {
                textSize = 13f; setTextColor(if (locked) Ui.WARN else Ui.DIM); layoutParams = LinearLayout.LayoutParams(dp(30), dp(34))
            })
            row.addView(android.widget.ImageView(this).apply {
                background = Ui.round(Ui.CHECK_B, dp(3f).toFloat(), 1, Ui.BORDER)
                setImageBitmap(layerThumb(index, frame))
                scaleType = android.widget.ImageView.ScaleType.FIT_CENTER
                layoutParams = LinearLayout.LayoutParams(dp(34), dp(34)).apply { setMargins(dp(2), 0, dp(8), 0) }
            })
            row.addView(TextView(this).apply {
                text = l["name"].toString(); maxLines = 1; textSize = 13f
                setTextColor(if (visible) Ui.TEXT else Ui.DIM); if (active) typeface = Typeface.DEFAULT_BOLD
            }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
            val badges = buildList {
                val d = (l["density"] as? Long) ?: 1L
                if (d > 1) add("${d}×")
                if (l["blend"] != "normal") add(l["blend"].toString())
                val o = (l["opacity"] as? Long) ?: 255L
                if (o < 255) add("${o * 100 / 255}%")
            }
            if (badges.isNotEmpty()) row.addView(TextView(this).apply { text = badges.joinToString(" "); textSize = 10f; setTextColor(Ui.ACCENT) })
            layerRows.addView(row, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT).apply { setMargins(0, 0, 0, dp(2)) })
        }
    }

    private fun layerThumb(index: Int, frame: Int): Bitmap? {
        val r = engine.execute("render_frame", mapOf("frame" to frame, "layer" to index, "density" to 1))
        val w = r.result.long("width")?.toInt() ?: return null
        val h = r.result.long("height")?.toInt() ?: return null
        val px = IntArray(w * h)
        val b = r.binary
        for (i in 0 until w * h) {
            val o = i * 4
            px[i] = ((b[o + 3].toInt() and 0xFF) shl 24) or ((b[o].toInt() and 0xFF) shl 16) or ((b[o + 1].toInt() and 0xFF) shl 8) or (b[o + 2].toInt() and 0xFF)
        }
        val small = Bitmap.createBitmap(px, w, h, Bitmap.Config.ARGB_8888)
        val s = max(1, 64 / max(w, h))
        return if (s > 1) Bitmap.createScaledBitmap(small, w * s, h * s, false) else small
    }

    /** Rename, opacity, blend, pixel size (density) and delete for one layer. */
    private fun layerProperties(l: Map<*, *>) {
        val index = (l["index"] as Long).toInt()
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(20), dp(8), dp(20), 0) }
        val name = EditText(this).apply { setText(l["name"].toString()); setTextColor(Ui.TEXT) }
        box.addView(label("Name")); box.addView(name)
        box.addView(label("Opacity"))
        val opacity = android.widget.SeekBar(this).apply { max = 255; progress = ((l["opacity"] as? Long) ?: 255L).toInt() }
        box.addView(opacity)
        val blends = arrayOf("normal", "multiply", "screen", "add", "overlay", "darken", "lighten", "soft_light")
        var blend = l["blend"]?.toString() ?: "normal"
        var density = ((l["density"] as? Long) ?: 1L).toInt()
        val blendRow = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val densRow = LinearLayout(this)
        fun redraw() {
            blendRow.removeAllViews()
            blends.toList().chunked(4).forEach { chunk ->
                blendRow.addView(LinearLayout(this).apply { chunk.forEach { b -> addView(chip(b, b == blend) { blend = b; redraw() }) }; setPadding(0, dp(3), 0, dp(3)) })
            }
            densRow.removeAllViews()
            listOf(1, 2, 4).forEach { d -> densRow.addView(chip(if (d == 1) "1× pixels" else "$d× (1/$d size)", d == density) { density = d; redraw() }) }
        }
        redraw()
        box.addView(label("Blend")); box.addView(HorizontalScrollView(this).apply { addView(blendRow) })
        box.addView(label("Pixel size")); box.addView(HorizontalScrollView(this).apply { addView(densRow) })
        AlertDialog.Builder(this).setTitle("Layer").setView(ScrollView(this).apply { addView(box) })
            .setPositiveButton("Apply") { _, _ ->
                engine.execute("layer_set", mapOf("layer" to index, "name" to name.text.toString().ifBlank { l["name"].toString() },
                    "opacity" to opacity.progress, "blend" to blend, "density" to density)).let { if (!it.ok) toast(it.errorMessage ?: "") }
            }
            .setNeutralButton("Delete") { _, _ -> engine.execute("layer_delete", mapOf("layer" to index)).let { if (!it.ok) toast(it.errorMessage ?: "") } }
            .setNegativeButton("Cancel", null).show()
    }

    // ---- color sheet --------------------------------------------------------------------

    /** HSV picker, material ramps (dark → light) and swatches in one bottom sheet. */
    private fun colorSheet() {
        val p = engine.execute("canvas_inspect", mapOf("detail" to "palette")).result
        val dialog = android.app.Dialog(this)
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(16), dp(12), dp(16), dp(16)); background = Ui.round(Ui.PANEL, dp(14f).toFloat()) }
        val picker = HsvPickerView(this, Ui.parse(canvas.color)) { pickColor(Ui.hex(it)) }
        box.addView(TextView(this).apply { text = "COLOR"; textSize = 11f; setTextColor(Ui.DIM); typeface = Typeface.DEFAULT_BOLD })
        box.addView(picker, LinearLayout.LayoutParams(MATCH_PARENT, dp(190)))
        fun swatch(hex: String) = View(this).apply {
            background = Ui.round(Ui.parse(hex), dp(4f).toFloat(), if (hex.equals(canvas.color, true)) dp(2) else 0, Color.WHITE)
            layoutParams = LinearLayout.LayoutParams(dp(30), dp(30)).apply { setMargins(dp(2), dp(2), dp(2), dp(2)) }
            setOnClickListener { pickColor(hex); dialog.dismiss() }
        }
        box.addView(TextView(this).apply { text = "MATERIAL RAMPS"; textSize = 11f; setTextColor(Ui.DIM); typeface = Typeface.DEFAULT_BOLD; setPadding(0, dp(12), 0, dp(4)) })
        val ramps = p.list("ramps").orEmpty().map { it as Map<*, *> }
        if (ramps.isEmpty()) box.addView(TextView(this).apply { text = "No ramps yet — a ramp is one material's tones, dark → light. The Shade tool steps along it."; setTextColor(Ui.DIM); textSize = 12f })
        for (r in ramps) {
            box.addView(LinearLayout(this).apply {
                gravity = Gravity.CENTER_VERTICAL
                addView(TextView(this@MainActivity).apply { text = r["name"].toString(); textSize = 12f; setTextColor(Ui.DIM); layoutParams = LinearLayout.LayoutParams(dp(64), WRAP_CONTENT) })
                (r["colors"] as List<*>).forEach { addView(swatch(it as String)) }
            })
        }
        box.addView(LinearLayout(this).apply {
            setPadding(0, dp(8), 0, 0)
            addView(chip("+ Ramp from color") {
                val input = EditText(this@MainActivity).apply { hint = "skin, hair, cloth…" }
                AlertDialog.Builder(this@MainActivity).setTitle("New ramp").setView(input)
                    .setPositiveButton("Create") { _, _ ->
                        val n = input.text.toString().trim().ifBlank { "material" }
                        engine.execute("color_ramp", mapOf("base" to canvas.color, "steps" to 5, "name" to n)); dialog.dismiss()
                    }.setNegativeButton("Cancel", null).show()
            })
            addView(chip("+ Swatch") { engine.execute("palette_set", mapOf("colors" to listOf(canvas.color), "mode" to "add")); dialog.dismiss() })
        })
        box.addView(TextView(this).apply { text = "SWATCHES"; textSize = 11f; setTextColor(Ui.DIM); typeface = Typeface.DEFAULT_BOLD; setPadding(0, dp(12), 0, dp(4)) })
        val colors = p.list("colors").orEmpty().mapNotNull { (it as? Map<*, *>)?.get("color") as? String }
        val grid = android.widget.GridLayout(this).apply { columnCount = 9 }
        colors.forEach { grid.addView(swatch(it)) }
        box.addView(grid)
        dialog.setContentView(ScrollView(this).apply { addView(box) })
        dialog.window?.apply {
            setBackgroundDrawable(android.graphics.drawable.ColorDrawable(Color.TRANSPARENT))
            setGravity(Gravity.BOTTOM)
            setLayout(MATCH_PARENT, WRAP_CONTENT)
        }
        dialog.show()
    }

    // ---- menus and files ----------------------------------------------------------------

    private fun overflow(anchor: View) {
        val menu = android.widget.PopupMenu(this, anchor)
        val items = listOf(
            "New 32 × 32", "New 64 × 64", "New 128 × 128", "Open .pxs…", "Save .pxs…", "Import image…", "Import reference…",
            "Export PNG…", "Export GIF…", "Outline (primary color)", "Outline (selective)", "Auto shade layer", "Pixel-perfect cleanup",
            "Review art…", "Form light layer", "Fix banding", "Selective anti-alias", "Import AI concept…",
        )
        items.forEachIndexed { i, s -> menu.menu.add(0, i, i, s) }
        menu.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                0 -> newProject(32)
                1 -> newProject(64)
                2 -> newProject(128)
                3 -> startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*"), REQ_OPEN)
                4 -> create("application/octet-stream", "${title.text}.pxs", REQ_SAVE)
                5 -> startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("image/png"), REQ_IMPORT)
                6 -> startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("image/png"), REQ_REFERENCE)
                7 -> create("image/png", "${title.text}.png", REQ_EXPORT_PNG)
                8 -> create("image/gif", "${title.text}.gif", REQ_EXPORT_GIF)
                9 -> engine.execute("outline", mapOf("color" to canvas.color, "mode" to "solid"))
                10 -> engine.execute("outline", mapOf("mode" to "selective"))
                11 -> engine.execute("auto_shade", mapOf("light" to "top-left", "tones" to 3))
                12 -> engine.execute("pixel_cleanup")
                13 -> reviewArt()
                14 -> engine.execute("shade_form", mapOf("light" to "top-left", "shadow_steps" to 2, "light_steps" to 2)).let { if (!it.ok) toast(it.errorMessage ?: "") }
                15 -> engine.execute("fix_banding")
                16 -> engine.execute("antialias", mapOf("amount" to 0.5))
                17 -> startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("image/*"), REQ_CONCEPT)
            }
            true
        }
        menu.show()
    }

    /** The critic: score, issues and the marked value map in one dialog. */
    private fun reviewArt() {
        val r = engine.execute("art_review", mapOf("frame" to activeFrame()))
        if (!r.ok) { toast(r.errorMessage ?: "nothing to review"); return }
        val res = r.result
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(16), dp(8), dp(16), dp(8)) }
        box.addView(android.widget.ImageView(this).apply {
            setImageBitmap(BitmapFactory.decodeByteArray(r.binary, 0, r.binary.size)); adjustViewBounds = true
        }, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
        val v = res["values"] as? Map<*, *>
        val lines = StringBuilder("Score ${res["score"]} / 100\nValue span ${v?.get("span")} (aim ≥ 55)\n\n")
        (res["issues"] as? List<*>).orEmpty().forEach { i -> (i as Map<*, *>).let { lines.append("• ${it["message"]}\n   → ${it["fix"]}\n") } }
        box.addView(TextView(this).apply { text = lines.toString(); setTextColor(Ui.TEXT); textSize = 13f; setPadding(0, dp(8), 0, 0) })
        AlertDialog.Builder(this).setTitle("Art review").setView(ScrollView(this).apply { addView(box) }).setPositiveButton("Close", null).show()
    }

    private fun newProject(size: Int) {
        engine.execute("project_new", mapOf("width" to size, "height" to size))
        canvas.post { canvas.refresh(); canvas.fit() }
    }

    private fun create(mime: String, name: String, request: Int) =
        startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mime).putExtra(Intent.EXTRA_TITLE, name), request)

    @Deprecated("Platform API; AndroidX Activity Result APIs are intentionally not used")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val uri = data?.data ?: return
        if (resultCode != RESULT_OK) return
        runCatching {
            when (requestCode) {
                REQ_OPEN -> {
                    val bytes = contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                    val r = engine.execute("project_load", payload = bytes)
                    if (r.ok) canvas.post { canvas.fit() } else toast(r.errorMessage ?: "cannot open")
                }
                REQ_IMPORT, REQ_REFERENCE -> {
                    val bytes = contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                    val args = linkedMapOf<String, Any?>("grid" to "auto", "ramps" to true)
                    if (requestCode == REQ_REFERENCE) args["reference"] = true else args["layer"] = "Imported"
                    val r = engine.execute("image_import", args, payload = bytes)
                    if (r.ok) {
                        val n = r.result["native"] as? Map<*, *>
                        toast("Imported ${n?.get("width")}×${n?.get("height")}, ${r.result["colors"]} colors")
                    } else toast(r.errorMessage ?: "cannot import")
                }
                REQ_CONCEPT -> {
                    // Decode any format Android knows (JPEG, PNG, WebP, HEIF) and hand the engine a PNG.
                    val raw = contentResolver.openInputStream(uri)!!.use { it.readBytes() }
                    val bmp = BitmapFactory.decodeByteArray(raw, 0, raw.size) ?: error("not an image")
                    val png = java.io.ByteArrayOutputStream().also { bmp.compress(Bitmap.CompressFormat.PNG, 100, it) }.toByteArray()
                    val r = engine.execute("concept_import", mapOf("colors" to 32, "layer" to "Concept"), payload = png)
                    if (r.ok) {
                        val sp = r.result["sprite"] as? Map<*, *>
                        canvas.post { canvas.fit() }
                        toast("Concept: ${sp?.get("width")}×${sp?.get("height")} px, source pixel ${r.result["cell"]} px")
                    } else toast(r.errorMessage ?: "cannot import")
                }
                REQ_SAVE -> write(uri, engine.execute("project_save").binary)
                REQ_EXPORT_PNG -> write(uri, engine.execute("export", mapOf("format" to "png", "scale" to 8)).binary)
                REQ_EXPORT_GIF -> write(uri, engine.execute("export", mapOf("format" to "gif", "scale" to 8)).binary)
            }
        }.onFailure { toast("File error: ${it.message}") }
    }

    private fun write(uri: android.net.Uri, bytes: ByteArray) {
        contentResolver.openOutputStream(uri, "wt")!!.use { it.write(bytes) }
        toast("Saved")
    }

    private fun agentDialog() {
        val running = app.gateway?.running == true
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(20), dp(10), dp(20), dp(10)) }
        fun line(text: String, color: Int = Ui.TEXT, mono: Boolean = false) = box.addView(TextView(this).apply {
            this.text = text
            setTextColor(color)
            setTextIsSelectable(true)
            if (mono) typeface = Typeface.MONOSPACE
            setPadding(0, dp(4), 0, dp(4))
        })
        if (!running) {
            line("Agent Mode lets AI agents (Claude Code, Antigravity, …) draw on this canvas through MCP. It listens on 127.0.0.1 only.", Ui.DIM)
            line("From a computer, connect over USB with:  adb forward tcp:38471 tcp:38471", Ui.DIM, mono = true)
        } else {
            val g = app.gateway!!
            g.connectionSnippets().forEach { (title, snippet) ->
                line(title, Ui.ACCENT)
                line(snippet, Ui.TEXT, mono = true)
            }
        }
        AlertDialog.Builder(this).setTitle(if (running) "Agent Mode is ON" else "Agent Mode")
            .setView(ScrollView(this).apply { addView(box) })
            .setPositiveButton(if (running) "Stop" else "Start") { _, _ ->
                if (running) AgentService.stop(this) else {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                        checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                    ) {
                        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 10)
                    }
                    AgentService.start(this)
                    main.postDelayed({ agentDialog(); refreshAll() }, 400)
                }
            }
            .setNeutralButton("Copy Claude Code command") { _, _ ->
                app.gateway?.connectionSnippets()?.values?.firstOrNull()?.let {
                    getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Pixel Studio MCP", it))
                    toast("Copied")
                }
            }
            .setNegativeButton("Close", null).show()
    }

    private fun toggle3d() {
        val show3d = voxels.visibility != View.VISIBLE
        voxels.visibility = if (show3d) View.VISIBLE else View.GONE
        canvas.visibility = if (show3d) View.GONE else View.VISIBLE
        if (!show3d) return
        val models = voxels.models()
        val items = (models + listOf("Load knight demo", "Play / pause walk")).toTypedArray()
        AlertDialog.Builder(this).setTitle("3D Voxel").setItems(items) { _, which ->
            when {
                which < models.size -> { voxels.model = models[which]; voxels.requestRender() }
                which == models.size -> loadKnight()
                else -> { voxels.model = "Knight"; voxels.animation = "walk"; voxels.togglePlay() }
            }
        }.show()
        voxels.requestRender()
    }

    private fun loadKnight() {
        toast("Building the knight…")
        Thread {
            assets.open("Knight.jsonl").bufferedReader().useLines { lines ->
                lines.filter { it.isNotBlank() }.forEach { engine.executeRaw(it.toByteArray()) }
            }
            main.post { voxels.model = "Diorama"; voxels.requestRender() }
        }.start()
    }

    private fun toast(msg: String) = Toast.makeText(this, msg, Toast.LENGTH_SHORT).show()
}

/** Saturation/value square over a hue strip; reports every change. */
class HsvPickerView(context: Context, initial: Int, private val onPick: (Int) -> Unit) : View(context) {
    private val hsv = FloatArray(3).also { Color.colorToHSV(initial, it) }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val ring = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; strokeWidth = 3f }
    private val hueH get() = height * 0.14f
    private val gap get() = height * 0.06f

    private var whiteToHue: android.graphics.Shader? = null
    private var shaderHue = -1f
    private var clearToBlack: android.graphics.Shader? = null
    private var hueStrip: android.graphics.Shader? = null

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        val sq = h - hueH - gap
        clearToBlack = android.graphics.LinearGradient(0f, 0f, 0f, sq, Color.TRANSPARENT, Color.BLACK, android.graphics.Shader.TileMode.CLAMP)
        hueStrip = android.graphics.LinearGradient(0f, 0f, w.toFloat(), 0f, IntArray(7) { Color.HSVToColor(floatArrayOf(it * 60f, 1f, 1f)) }, null, android.graphics.Shader.TileMode.CLAMP)
        shaderHue = -1f
        updateHueShader()
    }

    /** Rebuilt only when the hue changes (never inside onDraw). */
    private fun updateHueShader() {
        if (shaderHue == hsv[0] && whiteToHue != null) return
        shaderHue = hsv[0]
        whiteToHue = android.graphics.LinearGradient(0f, 0f, width.toFloat(), 0f, Color.WHITE, Color.HSVToColor(floatArrayOf(hsv[0], 1f, 1f)), android.graphics.Shader.TileMode.CLAMP)
    }

    override fun onDraw(c: Canvas) {
        val sq = height - hueH - gap
        paint.shader = whiteToHue
        c.drawRoundRect(0f, 0f, width.toFloat(), sq, 12f, 12f, paint)
        paint.shader = clearToBlack
        c.drawRoundRect(0f, 0f, width.toFloat(), sq, 12f, 12f, paint)
        paint.shader = hueStrip
        c.drawRoundRect(0f, sq + gap, width.toFloat(), height.toFloat(), 10f, 10f, paint)
        paint.shader = null
        val x = hsv[1] * width; val y = (1 - hsv[2]) * sq
        ring.color = Color.BLACK; c.drawCircle(x, y, 13f, ring)
        ring.color = Color.WHITE; c.drawCircle(x, y, 10f, ring)
        val hx = hsv[0] / 360f * width
        ring.color = Color.WHITE; c.drawRect(hx - 4, sq + gap, hx + 4, height.toFloat(), ring)
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        parent?.requestDisallowInterceptTouchEvent(true)
        val sq = height - hueH - gap
        if (e.y > sq + gap / 2) hsv[0] = (e.x / width).coerceIn(0f, 0.999f) * 360f
        else { hsv[1] = (e.x / width).coerceIn(0f, 1f); hsv[2] = 1f - (e.y / sq).coerceIn(0f, 1f) }
        updateHueShader()
        onPick(Color.HSVToColor(hsv))
        invalidate()
        if (e.actionMasked == MotionEvent.ACTION_UP) performClick()
        return true
    }

    override fun performClick(): Boolean = super.performClick()
}

// ═══ PixelCanvasView ═════════════════════════════════════════════════════════════════════════════

/**
 * Touch canvas: one finger draws, two fingers pinch-zoom and pan. A stroke is previewed
 * locally and committed as one engine command (one undo step) when the finger lifts.
 * Coordinates follow the active layer's pixel density (half/quarter-size pixels on 2×/4× layers).
 */
class PixelCanvasView(context: Context, private val engine: PixelEngine) : View(context) {
    var tool = Tool.PENCIL
    var color = "#1B1F2E"
    var onion = false
    var grid = true
    var brushSize = 1
    var pixelPerfect = true
    var mirrorX = false
    var mirrorY = false
    var filled = false
    var contiguous = true
    var shadeSteps = -1
    var density = 1
    var onColorPicked: (String) -> Unit = {}

    private var bitmap: Bitmap? = null
    private var docW = 32
    private var docH = 32
    private var frame = 0
    private var zoom = 0f
    private var panX = 0f
    private var panY = 0f
    private val stroke = LinkedHashSet<Long>()
    private val path = ArrayList<Pair<Int, Int>>()
    private var anchor: Pair<Int, Int>? = null
    private var current: Pair<Int, Int>? = null
    private var moveFrom: Pair<Float, Float>? = null
    private var moveDelta = Pair(0, 0)
    private var multiTouch = false
    private var lastFocus: Pair<Float, Float>? = null

    private val pixelPaint = Paint().apply { isFilterBitmap = false; isAntiAlias = false }
    private val checkerA = Paint().apply { color = Ui.CHECK_A }
    private val checkerB = Paint().apply { color = Ui.CHECK_B }
    private val preview = Paint()
    private val outline = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE; color = Ui.ACCENT; strokeWidth = 3f }
    private val guide = Paint().apply { style = Paint.Style.STROKE; color = 0xFFC08CFF.toInt(); strokeWidth = 2f; pathEffect = android.graphics.DashPathEffect(floatArrayOf(12f, 8f), 0f) }
    private val gridPaint = Paint().apply { color = Color.argb(22, 255, 255, 255) }
    private val frameShadow = Paint().apply { color = Color.argb(90, 0, 0, 0) }
    private val bitmapMatrix = Matrix()
    private val shapeRect = RectF()

    private val scaler = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            val newZoom = (zoom * d.scaleFactor).coerceIn(1f, 96f)
            panX = d.focusX - (d.focusX - panX) * newZoom / zoom
            panY = d.focusY - (d.focusY - panY) * newZoom / zoom
            zoom = newZoom
            invalidate()
            return true
        }
    })

    init {
        setBackgroundColor(Ui.CANVAS)
    }

    fun refresh() {
        val info = engine.execute("canvas_inspect").result
        docW = info.long("width")?.toInt() ?: docW
        docH = info.long("height")?.toInt() ?: docH
        frame = info.long("activeFrame")?.toInt() ?: 0
        val args = linkedMapOf<String, Any?>("frame" to frame)
        if (onion) args["onion"] = mapOf("before" to 1, "after" to 1, "opacity" to 90)
        val r = engine.execute("render_frame", args)
        val w = r.result.long("width")?.toInt() ?: return
        val h = r.result.long("height")?.toInt() ?: return
        val px = IntArray(w * h)
        val b = r.binary
        for (i in 0 until w * h) {
            val o = i * 4
            px[i] = ((b[o + 3].toInt() and 0xFF) shl 24) or ((b[o].toInt() and 0xFF) shl 16) or ((b[o + 1].toInt() and 0xFF) shl 8) or (b[o + 2].toInt() and 0xFF)
        }
        val bmp = bitmap?.takeIf { it.width == w && it.height == h } ?: Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        bmp.setPixels(px, 0, w, 0, 0, w, h)
        bitmap = bmp
        invalidate()
    }

    fun fit() {
        if (width == 0) return
        zoom = max(1f, floor(min(width * 0.9f / docW, height * 0.9f / docH)))
        panX = (width - docW * zoom) / 2f
        panY = (height - docH * zoom) / 2f
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) = fit()

    private fun toPixel(x: Float, y: Float) = Pair(floor((x - panX) / zoom * density).toInt(), floor((y - panY) / zoom * density).toInt())
    private fun inside(p: Pair<Int, Int>) = p.first in 0 until docW * density && p.second in 0 until docH * density
    private fun key(x: Int, y: Int) = (x.toLong() shl 32) or (y.toLong() and 0xFFFFFFFFL)

    private fun mirrored(x: Int, y: Int): List<Pair<Int, Int>> {
        val w = docW * density; val h = docH * density
        val out = mutableListOf(Pair(x, y))
        if (mirrorX) out += Pair(w - 1 - x, y)
        if (mirrorY) out += Pair(x, h - 1 - y)
        if (mirrorX && mirrorY) out += Pair(w - 1 - x, h - 1 - y)
        return out
    }

    private fun addLine(a: Pair<Int, Int>, b: Pair<Int, Int>) {
        var (x0, y0) = a
        val (x1, y1) = b
        val dx = abs(x1 - x0); val dy = -abs(y1 - y0)
        val sx = if (x0 < x1) 1 else -1; val sy = if (y0 < y1) 1 else -1
        var err = dx + dy
        while (true) {
            if (path.lastOrNull() != Pair(x0, y0)) path += Pair(x0, y0)
            mirrored(x0, y0).forEach { stroke += key(it.first, it.second) }
            if (x0 == x1 && y0 == y1) return
            val e2 = 2 * err
            if (e2 >= dy) { err += dy; x0 += sx }
            if (e2 <= dx) { err += dx; y0 += sy }
        }
    }

    /** Pixel-perfect pass (drops L-corner doubles), then mirror and brush size. */
    private fun finalPixels(): List<List<Int>> {
        var pts: List<Pair<Int, Int>> = path
        if (pixelPerfect && tool == Tool.PENCIL) {
            val out = ArrayList<Pair<Int, Int>>()
            for (p in pts) {
                if (out.size >= 2) {
                    val a = out[out.size - 2]; val b = out[out.size - 1]
                    if (abs(a.first - p.first) == 1 && abs(a.second - p.second) == 1 && (b.first == a.first || b.second == a.second)) out.removeAt(out.size - 1)
                }
                out += p
            }
            pts = out
        }
        val set = LinkedHashSet<Pair<Int, Int>>()
        for ((x, y) in pts) for (m in mirrored(x, y)) for (oy in 0 until brushSize) for (ox in 0 until brushSize) {
            set += Pair(m.first + ox - (brushSize - 1) / 2, m.second + oy - (brushSize - 1) / 2)
        }
        return set.filter(::inside).map { listOf(it.first, it.second) }
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        scaler.onTouchEvent(e)
        if (e.pointerCount > 1) {
            // Second finger cancels the stroke and switches to pan/zoom.
            multiTouch = true
            stroke.clear(); path.clear(); anchor = null; moveFrom = null; moveDelta = Pair(0, 0)
            val fx = (e.getX(0) + e.getX(1)) / 2; val fy = (e.getY(0) + e.getY(1)) / 2
            lastFocus?.let { (lx, ly) -> panX += fx - lx; panY += fy - ly }
            lastFocus = Pair(fx, fy)
            invalidate()
            return true
        }
        val p = toPixel(e.x, e.y)
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                multiTouch = false; lastFocus = null
                when (tool) {
                    Tool.PENCIL, Tool.ERASER, Tool.SHADE -> { stroke.clear(); path.clear(); addLine(p, p); current = p }
                    Tool.LINE, Tool.RECTANGLE, Tool.ELLIPSE -> { anchor = p; current = p }
                    Tool.MOVE -> { moveFrom = Pair(e.x, e.y); moveDelta = Pair(0, 0) }
                    Tool.FILL -> if (inside(p)) engine.execute("fill_region", mapOf("x" to p.first, "y" to p.second, "color" to color, "contiguous" to contiguous))
                    Tool.PICKER -> if (inside(p)) {
                        engine.execute("get_pixel", mapOf("x" to p.first / density, "y" to p.second / density, "composite" to true)).result.str("color")?.let(onColorPicked)
                    }
                }
            }
            MotionEvent.ACTION_MOVE -> if (!multiTouch) {
                when (tool) {
                    Tool.PENCIL, Tool.ERASER, Tool.SHADE -> current?.let { addLine(it, p) }
                    Tool.MOVE -> moveFrom?.let { (fx, fy) -> moveDelta = Pair(((e.x - fx) / zoom).toInt(), ((e.y - fy) / zoom).toInt()) }
                    else -> {}
                }
                current = p
            }
            MotionEvent.ACTION_UP -> {
                if (!multiTouch) commit(p)
                performClick()
            }
            MotionEvent.ACTION_CANCEL -> { stroke.clear(); path.clear(); anchor = null; moveFrom = null; moveDelta = Pair(0, 0) }
        }
        invalidate()
        return true
    }

    private fun commit(p: Pair<Int, Int>) {
        when (tool) {
            Tool.PENCIL, Tool.ERASER -> {
                val pixels = finalPixels()
                if (pixels.isNotEmpty()) engine.execute("set_pixels", mapOf("pixels" to pixels, "color" to if (tool == Tool.ERASER) null else color))
            }
            Tool.SHADE -> {
                val pixels = finalPixels()
                if (pixels.isNotEmpty()) {
                    val r = engine.execute("shade", mapOf("pixels" to pixels, "steps" to shadeSteps))
                    if (!r.ok) Toast.makeText(context, "Shade needs material ramps — add one in the color sheet", Toast.LENGTH_LONG).show()
                }
            }
            Tool.LINE -> anchor?.let { engine.execute("draw_line", mapOf("x0" to it.first, "y0" to it.second, "x1" to p.first, "y1" to p.second, "color" to color)) }
            Tool.RECTANGLE, Tool.ELLIPSE -> anchor?.let {
                engine.execute(if (tool == Tool.RECTANGLE) "draw_rectangle" else "draw_ellipse", mapOf("x" to min(it.first, p.first), "y" to min(it.second, p.second),
                    "width" to abs(p.first - it.first) + 1, "height" to abs(p.second - it.second) + 1, "color" to color, "filled" to filled))
            }
            Tool.MOVE -> {
                val (dx, dy) = moveDelta
                if (dx != 0 || dy != 0) engine.execute("move_region", mapOf("x" to 0, "y" to 0, "width" to docW * density, "height" to docH * density,
                    "dx" to dx * density, "dy" to dy * density, "mode" to "replace"))
            }
            else -> {}
        }
        stroke.clear(); path.clear(); anchor = null; moveFrom = null; moveDelta = Pair(0, 0)
    }

    override fun performClick(): Boolean = super.performClick()

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (zoom == 0f) fit()
        val w = docW * zoom; val h = docH * zoom
        canvas.drawRect(panX + 6, panY + 8, panX + w + 6, panY + h + 8, frameShadow)
        canvas.save()
        canvas.clipRect(panX, panY, panX + w, panY + h)
        val cell = 24f
        var y = 0f
        var row = 0
        while (y < h) {
            var x = 0f
            var col = 0
            while (x < w) {
                canvas.drawRect(panX + x, panY + y, panX + x + cell, panY + y + cell, if ((row + col) % 2 == 0) checkerA else checkerB)
                x += cell; col++
            }
            y += cell; row++
        }
        bitmap?.let {
            val s = zoom * docW / it.width
            bitmapMatrix.setScale(s, s)
            bitmapMatrix.postTranslate(panX + moveDelta.first * zoom, panY + moveDelta.second * zoom)
            canvas.drawBitmap(it, bitmapMatrix, pixelPaint)
        }
        val cz = zoom / density
        preview.color = when (tool) {
            Tool.ERASER -> Color.argb(120, 255, 255, 255)
            Tool.SHADE -> if (shadeSteps < 0) Color.argb(120, 20, 10, 40) else Color.argb(120, 255, 240, 200)
            else -> Ui.parse(color)
        }
        for (k in stroke) {
            val px = (k shr 32).toInt(); val py = k.toInt()
            val o = (brushSize - 1) / 2
            canvas.drawRect(panX + (px - o) * cz, panY + (py - o) * cz, panX + (px - o + brushSize) * cz, panY + (py - o + brushSize) * cz, preview)
        }
        val a = anchor; val c = current
        if (a != null && c != null) {
            shapeRect.set(panX + min(a.first, c.first) * cz, panY + min(a.second, c.second) * cz,
                panX + (max(a.first, c.first) + 1) * cz, panY + (max(a.second, c.second) + 1) * cz)
            when (tool) {
                Tool.RECTANGLE -> canvas.drawRect(shapeRect, outline)
                Tool.ELLIPSE -> canvas.drawOval(shapeRect, outline)
                else -> canvas.drawLine(panX + (a.first + .5f) * cz, panY + (a.second + .5f) * cz, panX + (c.first + .5f) * cz, panY + (c.second + .5f) * cz, outline)
            }
        }
        if (grid && cz >= 10) {
            for (gx in 0..docW * density) canvas.drawLine(panX + gx * cz, panY, panX + gx * cz, panY + h, gridPaint)
            for (gy in 0..docH * density) canvas.drawLine(panX, panY + gy * cz, panX + w, panY + gy * cz, gridPaint)
        }
        if (mirrorX) canvas.drawLine(panX + w / 2, panY, panX + w / 2, panY + h, guide)
        if (mirrorY) canvas.drawLine(panX, panY + h / 2, panX + w, panY + h / 2, guide)
        canvas.restore()
    }
}

// ═══ VoxelView ═══════════════════════════════════════════════════════════════════════════════════

/**
 * 3D pixel viewport: drag to orbit, pinch to zoom, optional animation playback. Frames are
 * rendered by the Rust voxel engine (PBR, AO, shadows, bloom) on a background thread.
 */
class VoxelView(context: Context, private val engine: PixelEngine) : View(context) {
    var model: String? = null
    var animation: String? = null
    private var yaw = 30.0
    private var pitch = 22.0
    private var zoom = 1.0
    private var frame = 0.0
    private var bitmap: Bitmap? = null
    private var lastX = 0f
    private var lastY = 0f
    private val main = Handler(Looper.getMainLooper())
    private val worker = Executors.newSingleThreadExecutor()
    private val pending = AtomicBoolean(false)
    private val paint = Paint().apply { isFilterBitmap = false }
    private var playing = false

    private val scaler = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScale(d: ScaleGestureDetector): Boolean {
            zoom = (zoom * d.scaleFactor).coerceIn(0.3, 6.0)
            requestRender()
            return true
        }
    })

    init {
        setBackgroundColor(Color.rgb(0x0E, 0x0D, 0x13))
    }

    fun models(): List<String> = engine.execute("voxel_inspect").result.list("models").orEmpty().map { (it as Map<*, *>)["name"] as String }

    fun togglePlay() {
        playing = !playing
        if (playing) tick()
    }

    private fun tick() {
        if (!playing) return
        frame = (frame + 1) % 8
        requestRender()
        main.postDelayed({ tick() }, 110)
    }

    fun requestRender() {
        val m = model ?: models().firstOrNull()?.also { model = it } ?: return
        if (width < 16 || !pending.compareAndSet(false, true)) return
        val (w, h) = (width / 2).coerceAtMost(720) to (height / 2).coerceAtMost(720)
        worker.execute {
            pending.set(false)
            val args = linkedMapOf<String, Any?>(
                "model" to m, "yaw" to yaw, "pitch" to pitch, "width" to w, "height" to h, "smoothness" to 0.0,
                "supersample" to 2, "shadows" to 1, "ground_shadow" to 0.35, "bloom" to 0.6, "margin" to (h * (1 - 1 / zoom)).coerceAtLeast(-h * 2.0) * -0.5 + 8,
                "background_gradient" to listOf("#3A332D", "#0E0D13"),
            )
            animation?.let { args["animation"] = it; args["frame"] = frame }
            val r = engine.execute("voxel_render", args)
            if (r.ok) {
                val bmp = BitmapFactory.decodeByteArray(r.binary, 0, r.binary.size)
                main.post { bitmap = bmp; invalidate() }
            }
        }
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) = requestRender()

    override fun onTouchEvent(e: MotionEvent): Boolean {
        scaler.onTouchEvent(e)
        if (e.pointerCount > 1) return true
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> { lastX = e.x; lastY = e.y }
            MotionEvent.ACTION_MOVE -> {
                yaw = (yaw + (e.x - lastX) * 0.4) % 360
                pitch = (pitch + (e.y - lastY) * 0.25).coerceIn(-80.0, 80.0)
                lastX = e.x; lastY = e.y
                requestRender()
            }
            MotionEvent.ACTION_UP -> performClick()
        }
        return true
    }

    override fun performClick(): Boolean = super.performClick()

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val b = bitmap ?: return
        val s = minOf(width.toFloat() / b.width, height.toFloat() / b.height)
        canvas.save()
        canvas.translate((width - b.width * s) / 2, (height - b.height * s) / 2)
        canvas.scale(s, s)
        canvas.drawBitmap(b, 0f, 0f, paint)
        canvas.restore()
    }
}

// ═══ AgentService ════════════════════════════════════════════════════════════════════════════════

/**
 * Agent Mode. Runs the MCP/REST gateway as a foreground service with a visible notification,
 * so the user always knows an agent may be connected and can stop it with one tap.
 */
class AgentService : Service() {
    companion object {
        private const val CHANNEL = "agent_mode"
        private const val NOTIFICATION_ID = 38471
        private const val ACTION_STOP = "com.pixel.studio.action.STOP_AGENT_MODE"

        fun start(context: Context) = context.startForegroundService(Intent(context, AgentService::class.java))
        fun stop(context: Context) = context.stopService(Intent(context, AgentService::class.java))
    }

    private val app get() = application as PixelStudioApplication

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        val gateway = app.startGateway()
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, getString(R.string.agent_channel), NotificationManager.IMPORTANCE_LOW))
        val stopIntent = PendingIntent.getService(
            this, 0, Intent(this, AgentService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE,
        )
        val openIntent = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val notification = Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_menu_edit)
            .setContentTitle(getString(R.string.agent_running))
            .setContentText(gateway.mcpUrl)
            .setContentIntent(openIntent)
            .setOngoing(true)
            .addAction(Notification.Action.Builder(null, getString(R.string.agent_stop), stopIntent).build())
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
        return START_STICKY
    }

    override fun onDestroy() {
        app.stopGateway()
        super.onDestroy()
    }
}
