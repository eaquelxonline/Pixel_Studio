package com.pixel.studio.android

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
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
import com.pixel.studio.json.list
import com.pixel.studio.json.long
import com.pixel.studio.json.str
import java.lang.ref.WeakReference

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
            assets.open("knight.jsonl").bufferedReader().useLines { lines ->
                lines.filter { it.isNotBlank() }.forEach { engine.executeRaw(it.toByteArray()) }
            }
            main.post { voxels.model = "Diorama"; voxels.requestRender() }
        }.start()
    }

    private fun toast(msg: String) = Toast.makeText(this, msg, Toast.LENGTH_SHORT).show()
}
