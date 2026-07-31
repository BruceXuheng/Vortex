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
import com.vortex.service.VortexVpnService
import com.vortex.ui.navigation.VortexRoutes
import com.vortex.ui.screens.home.HomeScreen
import com.vortex.ui.screens.logdetail.LogDetailScreen
import com.vortex.ui.theme.Vortex_appTheme

/**
 * 应用主 Activity，承载 Navigation 导航图。
 *
 * 同时作为 ADB 远程启动 VPN 的入口：接收 `com.vortex.action.START` /
 * `com.vortex.action.STOP` Intent，转发给 [VortexVpnService]。
 */
class MainActivity : ComponentActivity() {

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
     * ADB shell 无法直接 `startservice` 启动 VpnService（`BIND_VPN_SERVICE`
     * 权限保护），因此由 Activity 转发：收到 Intent 后以应用自身身份
     * 启动/停止 VpnService。
     */
    private fun handleIntent(intent: Intent?) {
        when (intent?.action) {
            VortexVpnService.ACTION_START_VPN -> {
                val serviceIntent = Intent(this, VortexVpnService::class.java).apply {
                    action = VortexVpnService.ACTION_START_VPN
                }
                startForegroundService(serviceIntent)
            }
            VortexVpnService.ACTION_STOP_VPN -> {
                val serviceIntent = Intent(this, VortexVpnService::class.java).apply {
                    action = VortexVpnService.ACTION_STOP_VPN
                }
                startService(serviceIntent)
            }
        }
    }
}
