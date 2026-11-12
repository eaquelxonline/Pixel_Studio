package com.pixel.studio.android

import android.app.Activity
import android.app.AlertDialog
import android.app.Application
import android.os.Handler
import android.os.Looper
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.AgentGateway
import com.pixel.studio.gateway.GatewayConfig
import com.pixel.studio.gateway.tools.Confirmer
import com.pixel.studio.gateway.tools.DirectoryWorkspace
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

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
