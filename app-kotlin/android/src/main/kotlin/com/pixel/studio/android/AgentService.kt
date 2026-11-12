package com.pixel.studio.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import com.pixel.studio.R

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
