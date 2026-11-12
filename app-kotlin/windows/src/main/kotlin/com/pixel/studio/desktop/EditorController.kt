package com.pixel.studio.desktop

import com.pixel.studio.bridge.EngineResponse
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.json.bool
import com.pixel.studio.json.list
import com.pixel.studio.json.long
import com.pixel.studio.json.obj
import com.pixel.studio.json.str
import java.awt.image.BufferedImage
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import javax.swing.SwingUtilities

enum class Tool(val label: String, val key: Char) {
    PENCIL("Pencil", 'B'), ERASER("Eraser", 'E'), LINE("Line", 'L'), RECTANGLE("Rectangle", 'R'),
    ELLIPSE("Ellipse", 'O'), FILL("Fill", 'G'), PICKER("Picker", 'I'),
}

data class LayerInfo(val index: Int, val name: String, val visible: Boolean, val locked: Boolean, val opacity: Int)
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
        val palette = engine.execute("palette_get").result.list("colors")?.map { (it as Map<*, *>)["color"] as String } ?: emptyList()
        doc = DocInfo(
            name = i.str("name") ?: "Untitled",
            width = i.long("width")!!.toInt(),
            height = i.long("height")!!.toInt(),
            revision = i.long("revision") ?: 0,
            layers = i.list("layers")!!.map {
                @Suppress("UNCHECKED_CAST") val l = it as Map<String, Any?>
                LayerInfo(l.long("index")!!.toInt(), l.str("name")!!, l.bool("visible")!!, l.bool("locked")!!, l.long("opacity")!!.toInt())
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
