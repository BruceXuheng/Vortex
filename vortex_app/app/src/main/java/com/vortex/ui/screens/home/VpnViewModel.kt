package com.vortex.ui.screens.home

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.VpnService
import androidx.core.content.ContextCompat
import androidx.lifecycle.ViewModel
import com.vortex.service.VortexVpnService
import com.vortex.service.VpnConfiguration
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * VPN 连接状态管理 ViewModel。
 *
 * 统一管理所有 VPN 启动/停止逻辑，包括：
 * - UI 按钮触发
 * - ADB Intent 触发
 * - App 默认自动连接
 *
 * 通信方式：
 * - ViewModel → Service：通过 [Context.startForegroundService] 发送 Action
 * - Service → ViewModel：通过 BroadcastReceiver 接收状态广播
 */
class VpnViewModel : ViewModel() {

    /**
     * VPN 连接状态。
     *
     * 只包含三个稳定态，过渡态由 [isBusy] 控制：
     * - [DISCONNECTED]：未连接
     * - [CONNECTED]：已连接
     * - [ERROR]：连接出错
     */
    enum class VpnState {
        DISCONNECTED,
        CONNECTED,
        ERROR
    }

    private val _vpnState = MutableStateFlow(VpnState.DISCONNECTED)

    /** 当前 VPN 连接状态。 */
    val vpnState: StateFlow<VpnState> = _vpnState.asStateFlow()

    private val _isBusy = MutableStateFlow(false)

    /** 是否正在执行连接/断开操作，用于禁用按钮防止重复点击。 */
    val isBusy: StateFlow<Boolean> = _isBusy.asStateFlow()

    private val _prepareIntent = MutableStateFlow<Intent?>(null)

    /** VPN 权限授权 Intent，非 null 时应启动系统授权对话框。 */
    val prepareIntent: StateFlow<Intent?> = _prepareIntent.asStateFlow()

    /** 等待 VPN 授权时暂存的配置。 */
    private var pendingConfig: VpnConfiguration? = null

    private var receiver: BroadcastReceiver? = null
    private var receiverContext: Context? = null

    /**
     * 绑定 Service 状态广播。
     *
     * 注册 BroadcastReceiver 监听 [VortexVpnService] 发出的状态变更广播，
     * 注册后主动查询 [VortexVpnService.isRunning] 同步当前状态。
     *
     * @param context 用于注册 BroadcastReceiver 的 Context
     */
    fun bindServiceState(context: Context) {
        receiverContext = context
        receiver = object : BroadcastReceiver() {
            override fun onReceive(ctx: Context, intent: Intent) {
                when (intent.getStringExtra("state")) {
                    "CONNECTED" -> {
                        _vpnState.value = VpnState.CONNECTED
                        _isBusy.value = false
                    }
                    "DISCONNECTED" -> {
                        _vpnState.value = VpnState.DISCONNECTED
                        _isBusy.value = false
                    }
                    "ERROR" -> {
                        _vpnState.value = VpnState.ERROR
                        _isBusy.value = false
                    }
                }
            }
        }
        ContextCompat.registerReceiver(
            context,
            receiver,
            IntentFilter("com.vortex.VPN_STATE_CHANGED"),
            ContextCompat.RECEIVER_NOT_EXPORTED
        )

        // 主动同步 Service 当前状态，避免广播丢失
        if (VortexVpnService.isRunning) {
            _vpnState.value = VpnState.CONNECTED
            _isBusy.value = false
        }
    }

    /**
     * 启动 VPN 连接（默认配置）。
     *
     * 若未获得 VPN 权限，会将 [prepareIntent] 设为系统授权 Intent，
     * 由 UI 层启动授权对话框；授权通过后再启动 Service。
     *
     * @param context 用于调用 [VpnService.prepare] 和 [Context.startForegroundService]
     */
    fun startVpn(context: Context) {
        startVpn(context, VpnConfiguration())
    }

    /**
     * 启动 VPN 连接（自定义配置）。
     *
     * @param context 用于调用 [VpnService.prepare] 和 [Context.startForegroundService]
     * @param config VPN 配置参数
     */
    fun startVpn(context: Context, config: VpnConfiguration) {
        if (VortexVpnService.isRunning) return
        _isBusy.value = true
        val prepareIntent = VpnService.prepare(context)
        if (prepareIntent != null) {
            pendingConfig = config
            _prepareIntent.value = prepareIntent
            return
        }
        launchVpnService(context, config)
    }

    /**
     * 停止 VPN 连接。
     *
     * @param context 用于发送停止 Action 到 Service
     */
    fun stopVpn(context: Context) {
        _isBusy.value = true
        val intent = Intent(context, VortexVpnService::class.java).apply {
            action = VortexVpnService.ACTION_STOP_VPN
        }
        context.startService(intent)
    }

    /**
     * VPN 权限授权通过后调用，启动 VPN Service。
     *
     * @param context 用于启动 Service
     */
    fun onVpnPermissionResult(context: Context) {
        _prepareIntent.value = null
        val config = pendingConfig ?: VpnConfiguration()
        pendingConfig = null
        launchVpnService(context, config)
    }

    /**
     * VPN 权限被用户拒绝后调用，恢复初始状态。
     */
    fun onVpnPermissionDenied() {
        _vpnState.value = VpnState.DISCONNECTED
        _isBusy.value = false
        _prepareIntent.value = null
        pendingConfig = null
    }

    private fun launchVpnService(context: Context, config: VpnConfiguration = VpnConfiguration()) {
        val intent = Intent(context, VortexVpnService::class.java).apply {
            action = VortexVpnService.ACTION_START_VPN
            putExtra(VortexVpnService.EXTRA_VPN_CONFIGURATION, config)
        }
        context.startForegroundService(intent)
    }

    override fun onCleared() {
        super.onCleared()
        receiver?.let { receiverContext?.unregisterReceiver(it) }
        receiver = null
        receiverContext = null
    }
}
