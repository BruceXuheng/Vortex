package com.vortex.ui.screens.home

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.VpnService
import androidx.core.content.ContextCompat
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.vortex.service.VortexVpnService
import com.vortex.service.VpnConfiguration
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * VPN 连接状态管理 ViewModel。
 *
 * 统一管理所有 VPN 启动/停止逻辑，包括：
 * - UI 按钮触发
 * - ADB Intent 触发
 * - App 默认自动连接
 *
 * 通信方式：
 * - 外部 → ViewModel：通过 [dispatch] 写入 [actionFlow]
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

    /**
     * 外部发来的 VPN 操作指令。
     *
     * @param action Action 常量（[VortexVpnService.ACTION_START_VPN] 等）
     * @param config VPN 配置，仅对 START 有意义
     */
    data class VpnAction(val action: String?, val config: VpnConfiguration? = null)

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

    private val _actionFlow = MutableSharedFlow<VpnAction>(extraBufferCapacity = 16)

    /** 外部写入的 VPN 操作指令流，ViewModel 自行消费。 */
    val actionFlow: SharedFlow<VpnAction> = _actionFlow.asSharedFlow()

    /** 等待 context 就绪的缓存 action。 */
    private var queuedAction: VpnAction? = null

    private var receiver: BroadcastReceiver? = null
    private var receiverContext: Context? = null

    init {
        // ViewModel 自行消费 actionFlow，彻底消除回调时序问题
        viewModelScope.launch {
            actionFlow.collect { action ->
                // context 可能还没准备好，缓存等 executeAction 调用时处理
                queuedAction = action
                tryExecuteQueuedAction()
            }
        }
    }

    /**
     * 提交 VPN 操作指令。
     *
     * 任何时刻都可安全调用，无需关心 UI 是否已就绪。
     *
     * @param action Action 常量
     * @param config VPN 配置
     */
    fun dispatch(action: String?, config: VpnConfiguration? = null) {
        _actionFlow.tryEmit(VpnAction(action, config))
    }

    /**
     * 绑定 Service 状态广播，并提供 context 用于后续 VPN 操作。
     *
     * 注册 BroadcastReceiver 监听 [VortexVpnService] 发出的状态变更广播，
     * 注册后主动查询 [VortexVpnService.isRunning] 同步当前状态，
     * 并尝试执行缓存的 pending action。
     *
     * @param context 用于注册 BroadcastReceiver 和启动 Service 的 Context
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

        // context 就绪后，执行缓存的操作
        tryExecuteQueuedAction()
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

    /** 尝试执行缓存的操作，需要 receiverContext 已就绪。 */
    private fun tryExecuteQueuedAction() {
        val action = queuedAction ?: return
        val ctx = receiverContext ?: return
        queuedAction = null
        when (action.action) {
            VortexVpnService.ACTION_STOP_VPN -> stopVpn(ctx)
            else -> startVpn(ctx, action.config ?: VpnConfiguration())
        }
    }

    override fun onCleared() {
        super.onCleared()
        receiver?.let { receiverContext?.unregisterReceiver(it) }
        receiver = null
        receiverContext = null
    }
}
