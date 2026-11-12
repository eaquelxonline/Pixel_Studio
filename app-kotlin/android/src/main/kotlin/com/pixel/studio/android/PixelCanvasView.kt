package com.pixel.studio.android

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.RectF
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.json.long
import com.pixel.studio.json.str
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

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
