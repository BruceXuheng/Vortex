package com.vortex

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.vortex.service.Cidr
import com.vortex.service.VortexVpnService
import com.vortex.service.VpnConfiguration
import com.vortex.ui.navigation.VortexRoutes
import com.vortex.ui.screens.home.HomeScreen
import com.vortex.ui.screens.logdetail.LogDetailScreen
import com.vortex.ui.theme.Vortex_appTheme
import java.net.InetAddress

/**
 * 应用主 Activity，承载 Navigation 导航图。
 *
 * 同时作为 ADB 远程启动 VPN 的入口：接收 `com.vortex.action.START` /
 * `com.vortex.action.STOP` Intent，统一委托给 [com.vortex.ui.screens.home.VpnViewModel] 处理。
 */
class MainActivity : ComponentActivity() {

    /** 由 HomeScreen 设置，将 Intent 事件转发给 ViewModel。 */
    var onIntentAction: ((action: String?, config: VpnConfiguration?) -> Unit)? = null

    /** handler 注册前缓存的 pending action，注册后自动补发。 */
    private var pendingAction: Pair<String?, VpnConfiguration?>? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            Vortex_appTheme {
                val navController = rememberNavController()
                NavHost(
                    navController = navController,
                    startDestination = VortexRoutes.HOME,
                    modifier = Modifier.fillMaxSize()
                ) {
                    composable(VortexRoutes.HOME) {
                        HomeScreen(
                            onNavigateToLog = {
                                navController.navigate(VortexRoutes.LOG_DETAIL_PAGE)
                            },
                            onActionReady = { handler ->
                                onIntentAction = handler
                                // handler 就绪后补发缓存的 pending action
                                pendingAction?.let { handler(it.first, it.second) }
                                pendingAction = null
                            }
                        )
                    }
                    composable(VortexRoutes.LOG_DETAIL_PAGE) {
                        LogDetailScreen(
                            onBack = {
                                navController.popBackStack()
                            }
                        )
                    }
                }
            }
        }
        handleIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    /**
     * 处理 Activity 启动 Intent，统一委托给 ViewModel。
     *
     * 若 handler 尚未注册（首次 onCreate 时 LaunchedEffect 还未执行），
     * 将 action 缓存到 [pendingAction]，等 handler 就绪后自动补发。
     *
     * - `ACTION_STOP_VPN`：停止 VPN
     * - `ACTION_START_VPN`：提取 extras 参数后启动 VPN
     * - 默认启动（桌面图标等）：自动连接 VPN
     */
    private fun handleIntent(intent: Intent?) {
        val action = intent?.action
        val pair = when (action) {
            VortexVpnService.ACTION_STOP_VPN -> action to null
            VortexVpnService.ACTION_START_VPN -> action to createConfig(intent)
            else -> VortexVpnService.ACTION_START_VPN to VpnConfiguration()
        }

        if (onIntentAction != null) {
            onIntentAction!!.invoke(pair.first, pair.second)
        } else {
            // handler 未就绪，缓存等待补发
            pendingAction = pair
        }
    }

    /**
     * 从 Intent extras 提取 VPN 配置参数。
     *
     * 支持的 extras：
     * - `dnsServers`（String[]）：自定义 DNS 服务器列表
     * - `routes`（String[]）：CIDR 格式路由规则列表
     *
     * @param intent 包含可选配置参数的 Intent
     * @return VPN 配置
     */
    private fun createConfig(intent: Intent): VpnConfiguration {
        val dnsStrings = intent.getStringArrayExtra(VpnConfiguration.EXTRA_DNS_SERVERS) ?: emptyArray()
        val routeStrings = intent.getStringArrayExtra(VpnConfiguration.EXTRA_ROUTES) ?: emptyArray()

        val dnsServers = dnsStrings.map { InetAddress.getByName(it) }.toTypedArray()
        val routes = routeStrings.map { Cidr.parse(it) }.toTypedArray()

        return VpnConfiguration(dnsServers, routes)
    }
}
