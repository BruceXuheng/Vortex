package com.vortex.ui.screens.home

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
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.vortex.ui.theme.Vortex_appTheme

@Composable
fun HomeScreen(
    modifier: Modifier = Modifier,
    onNavigateToLog: () -> Unit = {}
) {

    Column(
        modifier = modifier
            .fillMaxSize()
            .wrapContentSize(Alignment.TopCenter)
            .padding(vertical = 24.dp, horizontal = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally
    ) {

        Text("Vortex", fontSize = 32.sp)

        Spacer(modifier = Modifier.height(10.dp))

        Text("零 Root 反向 USB 全局流量代理", fontSize = 14.sp)

        Text("一线相连，流量入涡", fontSize = 14.sp)

        Spacer(modifier = Modifier.height(20.dp))

        VortexCardControl(
            onNavigateToLog = onNavigateToLog
        )

    }

}


@Composable
fun VortexCardControl(
    onNavigateToLog: () -> Unit = {}
) {
    var isConnected by remember { mutableStateOf(false) }

    Card(
        modifier = Modifier
            .padding(10.dp)
            .width(200.dp)
    ) {
        Column(
            modifier = Modifier
                .padding(16.dp)
                .width(200.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
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
                Text(if (isConnected) "已连接" else "未连接")
            }

            Spacer(modifier = Modifier.height(16.dp))

            Button(onClick = {
                isConnected = !isConnected
            }) {
                Text(if (isConnected) "断开" else "连接")
            }

            Spacer(modifier = Modifier.height(8.dp))

            // 查看日志按钮
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
