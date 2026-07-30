package com.vortex.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor
import android.util.Log
import com.vortex.R
import java.io.IOException

/**
 * Vortex VPN 前台服务。
 *
 * 负责建立 VPN 接口、管理生命周期，通过 [RelayConnection] 和 [PacketForwarder]
 * 将流量转发到 PC 端 Relay Server，并通过广播向 ViewModel 报告状态变更。
 */
class VortexVpnService : VpnService() {

    companion object {
        /** 启动 VPN 的 Action。 */
        const val ACTION_START_VPN = "com.vortex.action.START"

        /** 停止 VPN 的 Action。 */
        const val ACTION_STOP_VPN = "com.vortex.action.STOP"

        private const val TAG = "VortexVpnService"
        private const val NOTIFICATION_ID = 1
        private const val NOTIFICATION_CHANNEL_ID = "vortex_vpn"
    }

    /** VPN 接口的文件描述符，用于读写 IP 包。 */
    private var vpnInterface: ParcelFileDescriptor? = null
    /** 包转发器，负责 VPN fd 与 Relay Server 之间的双向 IP 包转发。 */
    private var packetForwarder: PacketForwarder? = null

    /**
     * 处理启动/停止 VPN 的 Intent。
     *
     * @return START_STICKY 保证服务被杀后自动重启
     */
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

    /** 启动 VPN：显示前台通知、建立接口、启动转发、广播状态。 */
    private fun startVpn() {
        showForegroundNotification()
        try {
            vpnInterface = configureVpn()
            startForwarding()
            broadcastState("CONNECTED")
        } catch (e: Exception) {
            Log.e(TAG, "VPN 启动失败", e)
            broadcastState("ERROR", e.message)
            stopSelf()
        }
    }

    /**
     * 建立 Relay 连接并启动包转发。
     *
     * @throws IOException 连接 Relay Server 失败时抛出
     */
    private fun startForwarding() {
        val relayConnection = RelayConnection()
        val clientId = relayConnection.connect()
        Log.d(TAG, "已连接 Relay Server，client_id = ${clientId.toLong() and 0xFFFFFFFFL}")

        packetForwarder = PacketForwarder(vpnInterface!!, relayConnection) { errorMsg ->
            Log.e(TAG, "转发异常: $errorMsg")
            broadcastState("ERROR", errorMsg)
        }
        packetForwarder?.start()
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

    /** 停止 VPN：停止转发、关闭接口、移除通知、广播状态。 */
    private fun stopVpn() {
        packetForwarder?.stop()
        packetForwarder = null
        vpnInterface?.close()
        vpnInterface = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        broadcastState("DISCONNECTED")
        stopSelf()
    }

    /** 服务销毁时清理转发器和 VPN 接口。 */
    override fun onDestroy() {
        packetForwarder?.stop()
        packetForwarder = null
        vpnInterface?.close()
        vpnInterface = null
        super.onDestroy()
    }
}
