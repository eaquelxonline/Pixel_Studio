package com.pixel.studio.android

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.os.Handler
import android.os.Looper
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.json.list
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

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
