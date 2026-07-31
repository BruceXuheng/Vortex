package com.vortex.ui.screens.home

import android.app.Activity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.VpnKey
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.Icon
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewmodel.compose.viewModel
import com.vortex.service.VpnConfiguration
import com.vortex.service.VortexVpnService
import com.vortex.ui.theme.Vortex_appTheme

/**
 * 应用首页，展示 VPN 连接控制卡片。
 *
 * @param modifier Modifier
 * @param viewModel VPN 状态管理 ViewModel
 * @param onNavigateToLog 导航到日志详情页的回调
 */
@Composable
fun HomeScreen(
    modifier: Modifier = Modifier,
    viewModel: VpnViewModel = viewModel(),
    onNavigateToLog: () -> Unit = {},
    /** 注册 Intent 动作处理器，由 MainActivity 调用以转发 ADB/自动连接事件。 */
    onActionReady: ((handler: (String?, VpnConfiguration?) -> Unit) -> Unit)? = null
) {
    val vpnState by viewModel.vpnState.collectAsState()
    val isBusy by viewModel.isBusy.collectAsState()
    val context = LocalContext.current
    val prepareIntent by viewModel.prepareIntent.collectAsState()

    val launcher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) {
        if (it.resultCode == Activity.RESULT_OK) {
            viewModel.onVpnPermissionResult(context)
        } else {
            viewModel.onVpnPermissionDenied()
        }
    }

    LaunchedEffect(prepareIntent) {
        prepareIntent?.let { launcher.launch(it) }
    }

    LaunchedEffect(Unit) {
        viewModel.bindServiceState(context)
    }

    // 注册 Intent 动作处理器，Activity 通过此回调转发 ADB/自动连接事件到 ViewModel
    LaunchedEffect(onActionReady) {
        onActionReady?.invoke { action, config ->
            when (action) {
                VortexVpnService.ACTION_STOP_VPN -> viewModel.stopVpn(context)
                else -> viewModel.startVpn(context, config ?: VpnConfiguration())
            }
        }
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .wrapContentSize(Alignment.TopCenter)
            .padding(vertical = 24.dp, horizontal = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {
        Text("Vortex", fontSize = 32.sp)
        Spacer(modifier = Modifier.height(8.dp))
        Text("零 Root 反向 USB 全局流量代理", fontSize = 14.sp)
        Text("一线相连，流量入涡", fontSize = 14.sp)
        Spacer(modifier = Modifier.height(8.dp))
        VortexCardControl(
            vpnState = vpnState,
            isBusy = isBusy,
            onStartVpn = { viewModel.startVpn(context) },
            onStopVpn = { viewModel.stopVpn(context) },
            onNavigateToLog = onNavigateToLog
        )
    }
}

/**
 * VPN 连接控制卡片。
 *
 * 包含状态显示、连接/断开按钮和日志跳转按钮。
 * 过渡期间按钮禁用，防止重复操作。
 *
 * @param vpnState 当前 VPN 状态
 * @param isBusy 是否正在执行过渡操作
 * @param onStartVpn 点击连接/重连的回调
 * @param onStopVpn 点击断开的回调
 * @param onNavigateToLog 跳转日志页的回调
 */
@Composable
fun VortexCardControl(
    vpnState: VpnViewModel.VpnState,
    isBusy: Boolean,
    onStartVpn: () -> Unit,
    onStopVpn: () -> Unit,
    onNavigateToLog: () -> Unit = {}
) {
    Card(
        modifier = Modifier
            .padding(10.dp)
            .width(200.dp)
    ) {
        Column(
            modifier = Modifier
                .padding(16.dp)
                .width(200.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Row(
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Icon(
                    tint = Color.Red,
                    imageVector = Icons.Default.VpnKey,
                    contentDescription = "VPN"
                )
                Spacer(modifier = Modifier.width(8.dp))
                Text("VPN 连接", fontSize = 18.sp)
            }

            Spacer(modifier = Modifier.height(8.dp))

            Row {
                Text("状态: ")
                Text(
                    when {
                        isBusy && vpnState == VpnViewModel.VpnState.DISCONNECTED -> "连接中..."
                        isBusy && vpnState == VpnViewModel.VpnState.CONNECTED -> "断开中..."
                        vpnState == VpnViewModel.VpnState.CONNECTED -> "已连接"
                        vpnState == VpnViewModel.VpnState.ERROR -> "错误"
                        else -> "未连接"
                    }
                )
            }

            Spacer(modifier = Modifier.height(8.dp))

            Button(
                onClick = {
                    when (vpnState) {
                        VpnViewModel.VpnState.DISCONNECTED,
                        VpnViewModel.VpnState.ERROR -> onStartVpn()
                        VpnViewModel.VpnState.CONNECTED -> onStopVpn()
                    }
                },
                enabled = !isBusy
            ) {
                Text(
                    when {
                        isBusy && vpnState == VpnViewModel.VpnState.DISCONNECTED -> "连接中..."
                        isBusy && vpnState == VpnViewModel.VpnState.CONNECTED -> "断开中..."
                        vpnState == VpnViewModel.VpnState.CONNECTED -> "断开"
                        vpnState == VpnViewModel.VpnState.ERROR -> "重连"
                        else -> "连接"
                    }
                )
            }

            Spacer(modifier = Modifier.height(8.dp))

            OutlinedButton(onClick = onNavigateToLog) {
                Text("查看日志")
            }
        }
    }
}

@Preview
@Composable
fun VortexPreview() {
    Vortex_appTheme {
        HomeScreen()
    }
}
