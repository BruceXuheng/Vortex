package com.vortex

import android.app.Activity
import android.content.Intent
import android.net.VpnService
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
import com.vortex.ui.screens.logdetail.LogDetailScreen
import com.vortex.ui.theme.Vortex_appTheme
import java.net.InetAddress

/**
 * 应用主 Activity，承载 Navigation 导航图。
 *
 * 同时作为 ADB 远程启动 VPN 的入口：接收 `com.vortex.action.START` /
 * `com.vortex.action.STOP` Intent，转发给 [VortexVpnService]。
 *
 * 对齐 Gnirehtet 设计：
 * - 从 Intent extras 提取 DNS 和路由参数，构造 [VpnConfiguration]
 * - 启动前检查 VPN 授权（[VpnService.prepare]），未授权则请求用户确认
 * - 授权通过后再启动 VpnService
 */
class MainActivity : ComponentActivity() {

    companion object {
        private const val TAG = "MainActivity"
        private const val VPN_REQUEST_CODE = 0
    }

    /** 等待 VPN 授权时暂存的配置。 */
    private var pendingConfig: VpnConfiguration? = null

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
     * 处理来自 ADB `am start` 的 START/STOP Intent。
     *
     * 对齐 Gnirehtet：
     * - START 时从 extras 提取 dnsServers/routes 参数
     * - 检查 VPN 授权状态，未授权则请求用户确认
     * - STOP 时直接停止 VPN
     */
    private fun handleIntent(intent: Intent?) {
        when (intent?.action) {
            VortexVpnService.ACTION_START_VPN -> {
                val config = createConfig(intent)
                startVpn(config)
            }
            VortexVpnService.ACTION_STOP_VPN -> {
                stopVpn()
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

    /**
     * 启动 VPN 连接。
     *
     * 对齐 Gnirehtet：先检查 VPN 授权，未授权则请求用户确认，
     * 授权通过后在 [onActivityResult] 中启动 Service。
     *
     * @param config VPN 配置参数
     */
    private fun startVpn(config: VpnConfiguration) {
        val vpnIntent = VpnService.prepare(this)
        if (vpnIntent == null) {
            Log.d(TAG, "VPN 已授权，直接启动")
            launchVpnService(config)
        } else {
            Log.w(TAG, "VPN 需要用户授权，请求中...")
            pendingConfig = config
            startActivityForResult(vpnIntent, VPN_REQUEST_CODE)
        }
    }

    /** 停止 VPN 服务。 */
    private fun stopVpn() {
        val serviceIntent = Intent(this, VortexVpnService::class.java).apply {
            action = VortexVpnService.ACTION_STOP_VPN
        }
        startService(serviceIntent)
    }

    /**
     * VPN 授权结果回调。
     *
     * 用户授权后使用暂存的配置启动 VPN Service。
     */
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == VPN_REQUEST_CODE) {
            if (resultCode == Activity.RESULT_OK) {
                pendingConfig?.let { launchVpnService(it) }
            } else {
                Log.w(TAG, "VPN 授权被拒绝")
            }
            pendingConfig = null
        }
    }

    /**
     * 启动 VPN 前台服务。
     *
     * 将 [VpnConfiguration] 作为 Parcelable extra 传入 Service Intent。
     *
     * @param config VPN 配置参数
     */
    private fun launchVpnService(config: VpnConfiguration) {
        val serviceIntent = Intent(this, VortexVpnService::class.java).apply {
            action = VortexVpnService.ACTION_START_VPN
            putExtra(VortexVpnService.EXTRA_VPN_CONFIGURATION, config)
        }
        startForegroundService(serviceIntent)
    }
}
