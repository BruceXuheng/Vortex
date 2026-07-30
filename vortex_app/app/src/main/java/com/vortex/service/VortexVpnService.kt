package com.vortex.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import com.vortex.R

/**
 * Vortex VPN 前台服务。
 *
 * 负责建立 VPN 接口、管理生命周期，并通过广播向 ViewModel 报告状态变更。
 * 后续将在此处对接 relay server 进行流量转发。
 */
class VortexVpnService : VpnService() {

    companion object {
        /** 启动 VPN 的 Action。 */
        const val ACTION_START_VPN = "com.vortex.action.START"

        /** 停止 VPN 的 Action。 */
        const val ACTION_STOP_VPN = "com.vortex.action.STOP"

        private const val NOTIFICATION_ID = 1
        private const val NOTIFICATION_CHANNEL_ID = "vortex_vpn"
    }

    private var vpnInterface: ParcelFileDescriptor? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_VPN -> startVpn()
            ACTION_STOP_VPN -> stopVpn()
        }
        return START_STICKY
    }

    /**
     * 配置并建立 VPN 接口。
     *
     * @return 建立成功的 [ParcelFileDescriptor]
     * @throws IllegalStateException VPN 接口建立失败时抛出
     */
    private fun configureVpn(): ParcelFileDescriptor {
        return Builder()
            .addAddress("10.0.0.2", 32)
            .addRoute("0.0.0.0", 0)
            .addDnsServer("8.8.8.8")
            .setMtu(16384)
            .setSession("Vortex VPN")
            .establish()
            ?: throw IllegalStateException("VPN 接口建立失败")
    }

    /** 启动 VPN：显示前台通知、建立接口、广播状态。 */
    private fun startVpn() {
        showForegroundNotification()
        try {
            vpnInterface = configureVpn()
            broadcastState("CONNECTED")
        } catch (e: Exception) {
            broadcastState("ERROR", e.message)
            stopSelf()
        }
        // TODO: 开始转发流量（后续对接 relay server）
    }

    /** 显示前台服务通知，满足 Android 前台服务要求。 */
    private fun showForegroundNotification() {
        val notificationManager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        val channel = NotificationChannel(
            NOTIFICATION_CHANNEL_ID,
            "Vortex VPN",
            NotificationManager.IMPORTANCE_LOW
        )
        notificationManager.createNotificationChannel(channel)
        val notification = Notification.Builder(this, NOTIFICATION_CHANNEL_ID)
            .setContentTitle("Vortex VPN")
            .setContentText("VPN 正在运行")
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .build()
        startForeground(NOTIFICATION_ID, notification)
    }

    /**
     * 广播 VPN 状态变更。
     *
     * @param state 状态字符串："CONNECTED"、"DISCONNECTED"、"ERROR"
     * @param message 附加信息，如错误描述
     */
    private fun broadcastState(state: String, message: String? = null) {
        val intent = Intent("com.vortex.VPN_STATE_CHANGED").apply {
            putExtra("state", state)
            message?.let { putExtra("message", it) }
        }
        sendBroadcast(intent)
    }

    /** 停止 VPN：关闭接口、移除通知、广播状态。 */
    private fun stopVpn() {
        vpnInterface?.close()
        vpnInterface = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        broadcastState("DISCONNECTED")
        stopSelf()
    }

    override fun onDestroy() {
        vpnInterface?.close()
        vpnInterface = null
        super.onDestroy()
    }
}
