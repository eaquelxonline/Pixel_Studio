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

// ═══ MainActivity ════════════════════════════════════════════════════════════════════════════════

/** Editor screen built from platform Views only (no AndroidX, no Compose). */
class MainActivity : Activity() {
    private companion object {
        const val REQ_SAVE = 1
        const val REQ_OPEN = 2
        const val REQ_EXPORT_PNG = 3
        const val REQ_EXPORT_GIF = 4
        const val ACCENT = 0xFF38E1FF.toInt()
        const val PANEL = 0xFF121833.toInt()
        const val TEXT = 0xFFE6EAFF.toInt()
        const val DIM = 0xFF8C95C4.toInt()
    }

    private val app get() = application as PixelStudioApplication
    private val engine get() = app.engine
    private lateinit var canvas: PixelCanvasView
    private lateinit var voxels: VoxelView
    private lateinit var stage: android.widget.FrameLayout
    private lateinit var paletteRow: LinearLayout
    private lateinit var frameLabel: TextView
    private lateinit var status: TextView
    private val toolButtons = LinkedHashMap<Tool, Button>()
    private val main = Handler(Looper.getMainLooper())
    private var refreshPosted = false
    private var playing: Runnable? = null
    private val listener: (Long) -> Unit = { scheduleRefresh() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        canvas = PixelCanvasView(this, engine).apply { onColorPicked = { selectColor(it) } }
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(0xFF0B1024.toInt()) }
        root.addView(topBar())
        voxels = VoxelView(this, engine).apply { visibility = View.GONE }
        stage = android.widget.FrameLayout(this).apply { addView(canvas); addView(voxels) }
        root.addView(stage, LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f))
        root.addView(scrollRow(toolRow()))
        paletteRow = LinearLayout(this).apply { setPadding(8, 4, 8, 4) }
        root.addView(scrollRow(paletteRow))
        root.addView(frameRow())
        status = TextView(this).apply { setTextColor(DIM); textSize = 11f; setPadding(16, 4, 16, 8) }
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

    private fun refreshAll() {
        canvas.refresh()
        val info = engine.execute("canvas_inspect").result
        val frames = info.list("frames")?.size ?: 1
        val active = info.long("activeFrame")?.toInt() ?: 0
        val layers = info.list("layers").orEmpty()
        val layerName = (layers.getOrNull(info.long("activeLayer")?.toInt() ?: 0) as? Map<*, *>)?.get("name") ?: "?"
        frameLabel.text = "Frame ${active + 1}/$frames"
        val agent = app.gateway?.let { "Agent Mode on" } ?: "Agent Mode off"
        status.text = "${info.str("name")} • ${info.long("width")}×${info.long("height")} • layer $layerName • rev ${info.long("revision")} • $agent"
        rebuildPalette()
    }

    // ---- UI building --------------------------------------------------------------------

    private fun button(text: String, onClick: () -> Unit) = Button(this).apply {
        this.text = text
        isAllCaps = false
        setTextColor(TEXT)
        textSize = 13f
        minWidth = 0; minimumWidth = 0
        setPadding(24, 8, 24, 8)
        background = GradientDrawable().apply { cornerRadius = 14f; setColor(0xFF1B2347.toInt()) }
        setOnClickListener { onClick() }
        layoutParams = LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT).apply { setMargins(6, 4, 6, 4) }
    }

    private fun scrollRow(content: View) = HorizontalScrollView(this).apply {
        isHorizontalScrollBarEnabled = false
        setBackgroundColor(PANEL)
        addView(content)
    }

    private fun topBar() = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(12, 8, 12, 8)
        setBackgroundColor(PANEL)
        addView(TextView(this@MainActivity).apply {
            text = "Pixel Studio"
            setTextColor(ACCENT)
            textSize = 18f
            typeface = Typeface.DEFAULT_BOLD
        }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        addView(button("↶") { engine.execute("history_undo") })
        addView(button("↷") { engine.execute("history_redo") })
        addView(button("Layers") { layersDialog() })
        addView(button("3D") { toggle3d() })
        addView(button("Agent") { agentDialog() })
        addView(button("☰") { fileMenu() })
    }

    private fun toolRow() = LinearLayout(this).apply {
        setPadding(8, 4, 8, 4)
        for (t in Tool.entries) {
            val b = button(t.name.lowercase().replaceFirstChar { it.uppercase() }) { selectTool(t) }
            toolButtons[t] = b
            addView(b)
        }
        addView(button("Fit") { canvas.fit() })
    }

    private fun frameRow() = LinearLayout(this).apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(8, 4, 8, 4)
        setBackgroundColor(PANEL)
        frameLabel = TextView(this@MainActivity).apply { setTextColor(TEXT); setPadding(12, 0, 12, 0) }
        addView(button("◀") { stepFrame(-1) })
        addView(frameLabel)
        addView(button("▶") { stepFrame(1) })
        addView(button("+") { engine.execute("frame_add", mapOf("index" to activeFrame() + 1)) })
        addView(button("⧉") { engine.execute("frame_duplicate") })
        addView(button("Play") { togglePlay() })
        addView(button("Onion") { canvas.onion = !canvas.onion; canvas.refresh() })
    }

    private fun selectTool(t: Tool) {
        canvas.tool = t
        toolButtons.forEach { (tool, b) -> b.setTextColor(if (tool == t) ACCENT else TEXT) }
    }

    private fun selectColor(hex: String) {
        canvas.color = hex
        if (canvas.tool == Tool.PICKER || canvas.tool == Tool.ERASER) selectTool(Tool.PENCIL)
        rebuildPalette()
    }

    private fun rebuildPalette() {
        paletteRow.removeAllViews()
        val colors = engine.execute("palette_get").result.list("colors").orEmpty().map { (it as Map<*, *>)["color"] as String }
        for (hex in colors) {
            paletteRow.addView(View(this).apply {
                background = GradientDrawable().apply {
                    cornerRadius = 8f
                    setColor(runCatching { Color.parseColor(hex.take(7)) }.getOrDefault(Color.MAGENTA))
                    if (hex.equals(canvas.color, ignoreCase = true)) setStroke(4, Color.WHITE)
                }
                layoutParams = LinearLayout.LayoutParams(72, 72).apply { setMargins(6, 6, 6, 6) }
                setOnClickListener { selectColor(hex) }
            })
        }
        paletteRow.addView(button("+ Color") { addColorDialog() })
    }

    private fun activeFrame() = engine.execute("canvas_inspect").result.long("activeFrame")?.toInt() ?: 0

    private fun stepFrame(delta: Int) {
        val frames = engine.execute("canvas_inspect").result.list("frames")?.size ?: 1
        engine.execute("frame_select", mapOf("frame" to Math.floorMod(activeFrame() + delta, frames)))
    }

    private fun togglePlay() {
        playing?.let { main.removeCallbacks(it); playing = null; return }
        val r = object : Runnable {
            override fun run() {
                stepFrame(1)
                val frames = engine.execute("canvas_inspect").result.list("frames").orEmpty()
                val d = ((frames.getOrNull(activeFrame()) as? Map<*, *>)?.get("duration") as? Long ?: 100L)
                main.postDelayed(this, d)
            }
        }
        playing = r
        main.post(r)
    }

    // ---- dialogs ------------------------------------------------------------------------

    private fun addColorDialog() {
        val input = EditText(this).apply { hint = "#RRGGBB"; inputType = InputType.TYPE_CLASS_TEXT }
        AlertDialog.Builder(this).setTitle("Add color").setView(input)
            .setPositiveButton("Add") { _, _ ->
                val hex = input.text.toString().trim().let { if (it.startsWith("#")) it else "#$it" }
                val r = engine.execute("palette_add", mapOf("colors" to listOf(hex)))
                if (r.ok) selectColor(hex.uppercase()) else toast(r.errorMessage ?: "invalid color")
            }.setNegativeButton("Cancel", null).show()
    }

    private fun layersDialog() {
        val info = engine.execute("canvas_inspect").result
        val layers = info.list("layers").orEmpty().map { it as Map<*, *> }.reversed()
        val names = layers.map { (if (it["visible"] == true) "◉ " else "○ ") + it["name"] + if (it["locked"] == true) " [locked]" else "" }.toTypedArray()
        AlertDialog.Builder(this).setTitle("Layers (tap to select)")
            .setItems(names) { _, which -> layerActions(layers[which]) }
            .setPositiveButton("New layer") { _, _ -> engine.execute("layer_create") }
            .setNegativeButton("Close", null).show()
    }

    private fun layerActions(layer: Map<*, *>) {
        val index = (layer["index"] as Long).toInt()
        engine.execute("layer_select", mapOf("layer" to index))
        val actions = arrayOf("Show / hide", "Lock / unlock", "Rename", "Move up", "Move down", "Delete")
        AlertDialog.Builder(this).setTitle(layer["name"].toString()).setItems(actions) { _, which ->
            when (which) {
                0 -> engine.execute("layer_visibility", mapOf("layer" to index, "visible" to (layer["visible"] != true)))
                1 -> engine.execute("layer_lock", mapOf("layer" to index, "locked" to (layer["locked"] != true)))
                2 -> {
                    val input = EditText(this).apply { setText(layer["name"].toString()) }
                    AlertDialog.Builder(this).setTitle("Rename").setView(input)
                        .setPositiveButton("OK") { _, _ -> engine.execute("layer_rename", mapOf("layer" to index, "name" to input.text.toString())) }.show()
                }
                3 -> engine.execute("layer_move", mapOf("layer" to index, "index" to index + 1))
                4 -> if (index > 0) engine.execute("layer_move", mapOf("layer" to index, "index" to index - 1))
                5 -> engine.execute("layer_delete", mapOf("layer" to index)).let { if (!it.ok) toast(it.errorMessage ?: "") }
            }
        }.show()
    }

    private fun fileMenu() {
        val items = arrayOf("New 16×16", "New 32×32", "New 64×64", "Open .pxs…", "Save .pxs…", "Export PNG (frame)…", "Export GIF…", "Delete current frame")
        AlertDialog.Builder(this).setItems(items) { _, which ->
            when (which) {
                0 -> newProject(16)
                1 -> newProject(32)
                2 -> newProject(64)
                3 -> startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*"), REQ_OPEN)
                4 -> create("application/octet-stream", "project.pxs", REQ_SAVE)
                5 -> create("image/png", "frame.png", REQ_EXPORT_PNG)
                6 -> create("image/gif", "animation.gif", REQ_EXPORT_GIF)
                7 -> engine.execute("frame_delete", mapOf("frame" to activeFrame())).let { if (!it.ok) toast(it.errorMessage ?: "") }
            }
        }.show()
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
                REQ_SAVE -> write(uri, engine.execute("project_save").binary)
                REQ_EXPORT_PNG -> write(uri, engine.execute("export_png", mapOf("scale" to 8)).binary)
                REQ_EXPORT_GIF -> write(uri, engine.execute("export_gif", mapOf("scale" to 8)).binary)
            }
        }.onFailure { toast("File error: ${it.message}") }
    }

    private fun write(uri: android.net.Uri, bytes: ByteArray) {
        contentResolver.openOutputStream(uri, "wt")!!.use { it.write(bytes) }
        toast("Saved")
    }

    private fun agentDialog() {
        val running = app.gateway?.running == true
        val box = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(40, 20, 40, 20) }
        fun line(text: String, color: Int = TEXT, mono: Boolean = false) = box.addView(TextView(this).apply {
            this.text = text
            setTextColor(color)
            setTextIsSelectable(true)
            if (mono) typeface = Typeface.MONOSPACE
            setPadding(0, 8, 0, 8)
        })
        if (!running) {
            line("Agent Mode lets AI agents (Claude Code, Antigravity, …) draw on this canvas through MCP. It listens on 127.0.0.1 only.", DIM)
            line("From a computer, connect over USB with:  adb forward tcp:38471 tcp:38471", DIM, mono = true)
        } else {
            val g = app.gateway!!
            g.connectionSnippets().forEach { (title, snippet) ->
                line(title, ACCENT)
                line(snippet, TEXT, mono = true)
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

// ═══ PixelCanvasView ═════════════════════════════════════════════════════════════════════════════

enum class Tool { PENCIL, ERASER, LINE, RECTANGLE, FILL, PICKER }

/**
 * Touch canvas: one finger draws, two fingers pinch-zoom and pan. A stroke is previewed
 * locally and committed as one `set_pixels` command (one undo step) when the finger lifts.
 */
class PixelCanvasView(context: Context, private val engine: PixelEngine) : View(context) {
    var tool = Tool.PENCIL
    var color = "#000000"
    var onion = false
    var onColorPicked: (String) -> Unit = {}

    private var bitmap: Bitmap? = null
    private var docW = 32
    private var docH = 32
    private var frame = 0
    private var zoom = 0f
    private var panX = 0f
    private var panY = 0f
    private val stroke = LinkedHashSet<Long>()
    private var anchor: Pair<Int, Int>? = null
    private var current: Pair<Int, Int>? = null
    private var multiTouch = false
    private var lastFocus: Pair<Float, Float>? = null

    private val pixelPaint = Paint().apply { isFilterBitmap = false; isAntiAlias = false }
    private val checkerA = Paint().apply { color = Color.rgb(0x2A, 0x2F, 0x45) }
    private val checkerB = Paint().apply { color = Color.rgb(0x22, 0x27, 0x3B) }
    private val preview = Paint()
    private val outline = Paint().apply { style = Paint.Style.STROKE; color = Color.rgb(0x38, 0xE1, 0xFF); strokeWidth = 2f }
    private val gridPaint = Paint().apply { color = Color.argb(28, 255, 255, 255) }
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
        setBackgroundColor(Color.rgb(0x08, 0x0C, 0x1C))
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
        zoom = max(1f, floor(min(width * 0.92f / docW, height * 0.92f / docH)))
        panX = (width - docW * zoom) / 2f
        panY = (height - docH * zoom) / 2f
        invalidate()
    }

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) = fit()

    private fun toPixel(x: Float, y: Float) = Pair(floor((x - panX) / zoom).toInt(), floor((y - panY) / zoom).toInt())
    private fun inside(p: Pair<Int, Int>) = p.first in 0 until docW && p.second in 0 until docH
    private fun key(x: Int, y: Int) = (x.toLong() shl 32) or (y.toLong() and 0xFFFFFFFFL)

    private fun addLine(a: Pair<Int, Int>, b: Pair<Int, Int>) {
        var (x0, y0) = a
        val (x1, y1) = b
        val dx = abs(x1 - x0); val dy = -abs(y1 - y0)
        val sx = if (x0 < x1) 1 else -1; val sy = if (y0 < y1) 1 else -1
        var err = dx + dy
        while (true) {
            stroke += key(x0, y0)
            if (x0 == x1 && y0 == y1) return
            val e2 = 2 * err
            if (e2 >= dy) { err += dy; x0 += sx }
            if (e2 <= dx) { err += dx; y0 += sy }
        }
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        scaler.onTouchEvent(e)
        if (e.pointerCount > 1) {
            // Second finger cancels the stroke and switches to pan/zoom.
            multiTouch = true
            stroke.clear(); anchor = null
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
                    Tool.PENCIL, Tool.ERASER -> { stroke.clear(); addLine(p, p); current = p }
                    Tool.LINE, Tool.RECTANGLE -> { anchor = p; current = p }
                    Tool.FILL -> if (inside(p)) engine.execute("fill_region", mapOf("x" to p.first, "y" to p.second, "color" to color))
                    Tool.PICKER -> if (inside(p)) {
                        engine.execute("get_pixel", mapOf("x" to p.first, "y" to p.second, "composite" to true)).result.str("color")?.let(onColorPicked)
                    }
                }
            }
            MotionEvent.ACTION_MOVE -> if (!multiTouch) {
                if (tool == Tool.PENCIL || tool == Tool.ERASER) current?.let { addLine(it, p) }
                current = p
            }
            MotionEvent.ACTION_UP -> {
                if (!multiTouch) commit(p)
                performClick()
            }
            MotionEvent.ACTION_CANCEL -> { stroke.clear(); anchor = null }
        }
        invalidate()
        return true
    }

    private fun commit(p: Pair<Int, Int>) {
        when (tool) {
            Tool.PENCIL, Tool.ERASER -> {
                val pixels = stroke.map { Pair((it shr 32).toInt(), it.toInt()) }.filter(::inside).map { listOf(it.first, it.second) }
                if (pixels.isNotEmpty()) engine.execute("set_pixels", mapOf("pixels" to pixels, "color" to if (tool == Tool.ERASER) null else color))
                stroke.clear()
            }
            Tool.LINE -> anchor?.let { engine.execute("draw_line", mapOf("x0" to it.first, "y0" to it.second, "x1" to p.first, "y1" to p.second, "color" to color)) }
            Tool.RECTANGLE -> anchor?.let {
                engine.execute("draw_rectangle", mapOf("x" to min(it.first, p.first), "y" to min(it.second, p.second),
                    "width" to abs(p.first - it.first) + 1, "height" to abs(p.second - it.second) + 1, "color" to color))
            }
            else -> {}
        }
        anchor = null
    }

    override fun performClick(): Boolean = super.performClick()

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (zoom == 0f) fit()
        val w = docW * zoom; val h = docH * zoom
        canvas.save()
        canvas.clipRect(panX, panY, panX + w, panY + h)
        val cell = max(8f, zoom / 2)
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
            bitmapMatrix.setScale(zoom, zoom)
            bitmapMatrix.postTranslate(panX, panY)
            canvas.drawBitmap(it, bitmapMatrix, pixelPaint)
        }
        preview.color = if (tool == Tool.ERASER) Color.argb(120, 255, 255, 255) else runCatching { Color.parseColor(color) }.getOrDefault(Color.MAGENTA)
        for (k in stroke) {
            val px = (k shr 32).toInt(); val py = k.toInt()
            canvas.drawRect(panX + px * zoom, panY + py * zoom, panX + (px + 1) * zoom, panY + (py + 1) * zoom, preview)
        }
        val a = anchor; val c = current
        if (a != null && c != null) {
            shapeRect.set(panX + min(a.first, c.first) * zoom, panY + min(a.second, c.second) * zoom,
                panX + (max(a.first, c.first) + 1) * zoom, panY + (max(a.second, c.second) + 1) * zoom)
            if (tool == Tool.RECTANGLE) canvas.drawRect(shapeRect, outline)
            else canvas.drawLine(panX + (a.first + .5f) * zoom, panY + (a.second + .5f) * zoom, panX + (c.first + .5f) * zoom, panY + (c.second + .5f) * zoom, outline)
        }
        if (zoom >= 10) {
            for (gx in 0..docW) canvas.drawLine(panX + gx * zoom, panY, panX + gx * zoom, panY + h, gridPaint)
            for (gy in 0..docH) canvas.drawLine(panX, panY + gy * zoom, panX + w, panY + gy * zoom, gridPaint)
        }
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
