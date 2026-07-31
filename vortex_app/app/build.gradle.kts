import java.time.LocalDate
import java.time.format.DateTimeFormatter

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.vortex"
    compileSdk {
        version = release(37) {
            minorApiLevel = 1
        }
    }

    defaultConfig {
        applicationId = "com.vortex"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        create("release") {
            storeFile = rootProject.file(System.getenv("KEYSTORE_PATH") ?: "secrets/vortex_app.jks")
            storePassword = System.getenv("KEYSTORE_PASSWORD") ?: providers.gradleProperty("keystore.password").getOrElse("")
            keyAlias = System.getenv("KEY_ALIAS") ?: providers.gradleProperty("keystore.alias").getOrElse("key0")
            keyPassword = System.getenv("KEY_PASSWORD") ?: providers.gradleProperty("keystore.keyPassword").getOrElse("")
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
        }
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("release")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    buildFeatures {
        compose = true
    }

    /** 自定义 APK 输出文件名：vortex_app_{buildType}_v{versionName}_{date}.apk */
    androidComponents {
        onVariants { variant ->
            variant.outputs.forEach { output ->
                val date = LocalDate.now().format(DateTimeFormatter.ofPattern("yyyyMMdd"))
                val buildType = variant.buildType ?: "unknown"
                val version = variant.outputs.first().versionName.getOrElse("0.0.0")
                output.outputFileName.set("vortex_app_${buildType}_v${version}_${date}.apk")
            }
        }
    }
}

/** 构建完成后将 APK 拷贝到项目根 output/ 目录，统一产物输出位置。 */
tasks.register("copyApkToOutput") {
    group = "build"
    description = "将 Release APK 拷贝到 output/ 目录"
    dependsOn("assembleRelease")

    val apkDir = layout.projectDirectory.dir("build/outputs/apk/release")
    val outputDir = layout.projectDirectory.dir("../../output")

    doLast {
        outputDir.asFile.mkdirs()
        apkDir.asFile.listFiles()?.filter { it.extension == "apk" }?.forEach { apk ->
            val dest = File(outputDir.asFile, apk.name)
            apk.copyTo(dest, overwrite = true)
            logger.lifecycle("APK 已拷贝到: ${dest.absolutePath}")
        }
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    testImplementation(libs.junit)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(libs.androidx.junit)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
    debugImplementation(libs.androidx.compose.ui.tooling)
}
