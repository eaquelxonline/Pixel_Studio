package com.pixel.studio.desktop

import com.pixel.studio.json.str
import java.awt.BasicStroke
import java.awt.Color
import java.awt.Cursor
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.Point
import java.awt.RenderingHints
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.awt.event.MouseWheelEvent
import javax.swing.JPanel
import javax.swing.SwingUtilities
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

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
