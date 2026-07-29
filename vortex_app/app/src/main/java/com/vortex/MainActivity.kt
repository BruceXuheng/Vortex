package com.vortex

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.vortex.ui.navigation.VortexRoutes
import com.vortex.ui.screens.home.HomeScreen
import com.vortex.ui.screens.logdetail.LogDetailScreen
import com.vortex.ui.theme.Vortex_appTheme

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
    }
}
