package com.vortex

import android.content.Intent
import android.os.Bundle
import android.util.Log
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
import com.vortex.ui.screens.home.VpnViewModel
import com.vortex.ui.screens.logdetail.LogDetailScreen
import com.vortex.ui.theme.Vortex_appTheme
import java.net.InetAddress

/**
 * 应用主 Activity，承载 Navigation 导航图。
 *
 * 同时作为 ADB 远程启动 VPN 的入口：接收 `com.vortex.action.START` /
 * `com.vortex.action.STOP` Intent，通过 [VpnViewModel.dispatch] 委托给 ViewModel 处理。
 */
class MainActivity : ComponentActivity() {

    companion object {
        private const val TAG = "MainActivity"
    }

    private val viewModel = VpnViewModel()

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
                            viewModel = viewModel,
                            onNavigateToLog = {
                                navController.navigate(VortexRoutes.LOG_DETAIL_PAGE)
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
     * 处理 Activity 启动 Intent，通过 [VpnViewModel.dispatch] 委托给 ViewModel。
     *
     * dispatch 写入 SharedFlow，ViewModel 自行 coroutine 消费执行，
     * 无需关心 UI 是否已组合完成，彻底消除时序问题。
     *
     * - `ACTION_STOP_VPN`：停止 VPN
     * - `ACTION_START_VPN`：提取 extras 参数后启动 VPN
     * - 默认启动（桌面图标等）：自动连接 VPN
     */
    private fun handleIntent(intent: Intent?) {
        val action = intent?.action
        Log.i(TAG, "handleIntent: action=$action, intent=$intent")
        when (action) {
            VortexVpnService.ACTION_STOP_VPN -> {
                viewModel.dispatch(action)
            }
            VortexVpnService.ACTION_START_VPN -> {
                viewModel.dispatch(action, createConfig(intent))
            }
            else -> {
                // 默认启动（桌面图标等），自动连接 VPN
                viewModel.dispatch(VortexVpnService.ACTION_START_VPN, VpnConfiguration())
            }
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
