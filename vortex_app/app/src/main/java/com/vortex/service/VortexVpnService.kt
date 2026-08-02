package com.vortex.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.LinkAddress
import android.net.LinkProperties
import android.net.Network
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import com.vortex.R
import java.io.IOException
import java.net.InetAddress

/**
 * Vortex VPN 前台服务。
 *
 * 负责建立 VPN 接口、管理生命周期，通过 [RelayConnection] 和 [PacketForwarder]
 * 将流量转发到 PC 端 Relay Server，并通过广播向 ViewModel 报告状态变更。
 *
 * 对齐 Gnirehtet 设计：
 * - 支持 [VpnConfiguration] 可配置参数（DNS、路由）
 * - VPN 授权检查（[VpnService.prepare]）
 * - 阻塞模式 I/O（setBlocking）
 * - API 22+ 设置底层网络（setUnderlyingNetworks）
 */
class VortexVpnService : VpnService() {

    companion object {
        /** 启动 VPN 的 Action。 */
        const val ACTION_START_VPN = "com.vortex.action.START"

        /** 停止 VPN 的 Action。 */
        const val ACTION_STOP_VPN = "com.vortex.action.STOP"

        /** Service Intent 中传递 [VpnConfiguration] 的键。 */
        const val EXTRA_VPN_CONFIGURATION = "vpnConfiguration"

        /** VPN 隧道虚拟地址。 */
        private val VPN_ADDRESS = InetAddress.getByName("10.0.0.2")

        /**
         * MTU 大小。
         *
         * Gnirehtet 的经验值 0x4000 (16384) 性能最佳，
         * 过高（0x8000+）或过低（1500）都会导致性能下降。
         */
        private const val MTU = 0x4000

        private const val TAG = "VortexVpnService"
        private const val NOTIFICATION_ID = 1
        private const val NOTIFICATION_CHANNEL_ID = "vortex_vpn"

        /**
         * VPN 是否正在运行（静态标志）。
         *
         * 供 [com.vortex.ui.screens.home.VpnViewModel] 在绑定广播时
         * 主动查询当前状态，避免广播丢失导致 UI 不同步。
         */
        @Volatile
        var isRunning: Boolean = false
            private set
    }

    /** VPN 接口的文件描述符，用于读写 IP 包。 */
    private var vpnInterface: ParcelFileDescriptor? = null
    /** 包转发器，负责 VPN fd 与 Relay Server 之间的双向 IP 包转发。 */
    private var packetForwarder: PacketForwarder? = null

    /**
     * 处理启动/停止 VPN 的 Intent。
     *
     * @return START_NOT_STICKY —— 服务被杀后不自动重启，对齐 Gnirehtet 行为
     */
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_VPN -> {
                if (running) {
                    Log.d(TAG, "VPN 已在运行，忽略 START 请求")
                } else {
                    val config = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        intent.getParcelableExtra(EXTRA_VPN_CONFIGURATION, VpnConfiguration::class.java)
                    } else {
                        @Suppress("DEPRECATION")
                        intent.getParcelableExtra(EXTRA_VPN_CONFIGURATION)
                    }
                    startVpn(config ?: VpnConfiguration())
                }
            }
            ACTION_STOP_VPN -> {
                // closeVpn 涉及网络 I/O（wakeUpReadWorkaround 的 DatagramSocket.send），
                // 主线程执行会触发 NetworkOnMainThreadException，必须在后台线程关闭
                Thread { closeVpn() }.start()
            }
        }
        return START_NOT_STICKY
    }

    /** VPN 是否正在运行（委托给静态标志）。 */
    private val running: Boolean
        get() = isRunning

    /**
     * 配置并建立 VPN 接口。
     *
     * 对齐 Gnirehtet：
     * - 使用 [VpnConfiguration] 中的 DNS 和路由
     * - 设置阻塞模式 [setBlocking] 以使用同步 I/O
     * - API 22+ 调用 [setUnderlyingNetworks] 通知系统网络可用
     *
     * @param config VPN 配置参数
     * @return 建立成功的 [ParcelFileDescriptor]
     * @throws IllegalStateException VPN 接口建立失败时抛出
     */
    private fun configureVpn(config: VpnConfiguration): ParcelFileDescriptor {
        val builder = Builder()
            .addAddress(VPN_ADDRESS, 32)
            .setSession("Vortex VPN")
            .setMtu(MTU)

        // 路由规则：空则全局代理
        if (config.routes.isEmpty()) {
            builder.addRoute("0.0.0.0", 0)
        } else {
            for (route in config.routes) {
                builder.addRoute(route.address, route.prefixLength)
            }
        }

        // DNS 服务器：空则使用 Google DNS
        if (config.dnsServers.isEmpty()) {
            builder.addDnsServer("8.8.8.8")
        } else {
            for (dns in config.dnsServers) {
                builder.addDnsServer(dns)
            }
        }

        // 阻塞模式：同步 I/O 避免 polling
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP_MR1) {
            builder.setBlocking(true)
        }

        val pfd = builder.establish()
            ?: throw IllegalStateException("VPN 接口建立失败")

        // API 22+ 设置底层网络，使应用知道网络可用
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP_MR1) {
            setAsUnderlyingNetwork()
        }

        return pfd
    }

    /** 启动 VPN：显示前台通知、建立接口、启动转发、广播状态。 */
    private fun startVpn(config: VpnConfiguration) {
        showForegroundNotification()
        isRunning = true
        try {
            vpnInterface = configureVpn(config)
            startForwarding()
            broadcastState("CONNECTED")
        } catch (e: Exception) {
            Log.e(TAG, "VPN 启动失败", e)
            broadcastState("ERROR", e.message)
            closeVpn()
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
     * API 22+ 将 VPN 网络设为底层网络。
     *
     * 对齐 Gnirehtet：使应用知道 VPN 网络可用，
     * 否则某些应用可能认为无网络而拒绝请求。
     */
    private fun setAsUnderlyingNetwork() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.LOLLIPOP_MR1) {
            return
        }
        val vpnNetwork = findVpnNetwork()
        if (vpnNetwork != null) {
            setUnderlyingNetworks(arrayOf(vpnNetwork))
        }
    }

    /**
     * 在系统网络列表中查找 VPN 网络。
     *
     * 通过匹配 VPN 虚拟地址 [VPN_ADDRESS] 来识别。
     *
     * @return VPN 网络，未找到返回 null
     */
    private fun findVpnNetwork(): Network? {
        val cm = getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
        for (network in cm.allNetworks) {
            val linkProperties = cm.getLinkProperties(network) ?: continue
            if (linkProperties.linkAddresses.any { it.address == VPN_ADDRESS }) {
                return network
            }
        }
        return null
    }

    /**
     * 广播 VPN 状态变更。
     *
     * @param state 状态字符串："CONNECTED"、"DISCONNECTED"、"ERROR"
     * @param message 附加信息，如错误描述
     */
    private fun broadcastState(state: String, message: String? = null) {
        Log.i(TAG, "broadcastState: state=$state, message=$message")
        val intent = Intent("com.vortex.VPN_STATE_CHANGED").apply {
            putExtra("state", state)
            message?.let { putExtra("message", it) }
        }
        sendBroadcast(intent)
    }

    /** 关闭 VPN：停止转发、关闭接口、移除通知、广播状态。 */
    private fun closeVpn() {
        synchronized(this) {
            if (!running) return
            isRunning = false
        }

        try {
            packetForwarder?.stop()
            packetForwarder = null
            vpnInterface?.close()
            vpnInterface = null
        } catch (e: IOException) {
            Log.w(TAG, "关闭 VPN 文件描述符异常", e)
        }

        stopForeground(STOP_FOREGROUND_REMOVE)
        broadcastState("DISCONNECTED")
        stopSelf()
    }

    /** 服务销毁时清理转发器和 VPN 接口。 */
    override fun onDestroy() {
        closeVpn()
        super.onDestroy()
    }
}
