import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
    id("com.google.gms.google-services")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val signingProperties = Properties().apply {
    val propFile = rootProject.file("key.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

fun readSigningValue(propertyName: String, envName: String): String? {
    return (signingProperties.getProperty(propertyName) ?: System.getenv(envName))
        ?.takeIf { it.isNotBlank() }
}

val releaseKeystorePath = readSigningValue("storeFile", "ANDROID_KEYSTORE_PATH")
val releaseKeystoreFile = releaseKeystorePath?.let(::file)?.takeIf { it.exists() }
val releaseStorePassword = readSigningValue("storePassword", "ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = readSigningValue("keyAlias", "ANDROID_KEY_ALIAS")
val releaseKeyPassword = readSigningValue("keyPassword", "ANDROID_KEY_PASSWORD")
val hasReleaseSigning = listOf(
    releaseKeystoreFile,
    releaseStorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
).all { it != null }

if (releaseKeystorePath != null && releaseKeystoreFile == null) {
    logger.warn("Android release signing skipped because the keystore file does not exist: $releaseKeystorePath")
}

android {
    compileSdk = 36
    namespace = "com.fancymumble.app"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.fancymumble.app"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = releaseKeystoreFile
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    kotlinOptions {
        jvmTarget = "11"
    }
    buildFeatures {
        buildConfig = true
    }
    testOptions {
        unitTests.isIncludeAndroidResources = true
    }
}

rust {
    rootDirRel = "../../../"
}

// Workaround: Tauri CLI may create libc++_shared.so symlinks pointing to the
// wrong NDK version. This task resolves NDK_HOME and copies the correct
// libc++_shared.so into jniLibs, overwriting the stale symlink.
val fixLibcppShared by tasks.registering {
    val ndkHome = System.getenv("NDK_HOME")
        ?: System.getenv("ANDROID_NDK_HOME")
        ?: System.getenv("ANDROID_NDK_ROOT")

    doLast {
        if (ndkHome == null) {
            logger.warn("NDK_HOME/ANDROID_NDK_HOME/ANDROID_NDK_ROOT not set, skipping libc++_shared.so fix")
            return@doLast
        }

        val abiToTriple = mapOf(
            "arm64-v8a" to "aarch64-linux-android",
            "armeabi-v7a" to "arm-linux-androideabi",
            "x86" to "i686-linux-android",
            "x86_64" to "x86_64-linux-android",
        )

        val hostTag = when {
            System.getProperty("os.name").lowercase().contains("win") -> "windows-x86_64"
            System.getProperty("os.name").lowercase().contains("mac") -> "darwin-x86_64"
            else -> "linux-x86_64"
        }

        val jniLibsDir = file("src/main/jniLibs")
        for ((abi, triple) in abiToTriple) {
            val src = file("$ndkHome/toolchains/llvm/prebuilt/$hostTag/sysroot/usr/lib/$triple/libc++_shared.so")
            if (!src.exists()) continue
            val dst = file("$jniLibsDir/$abi/libc++_shared.so")
            if (!dst.parentFile.exists()) continue
            // Delete the (possibly stale) symlink or file, then copy
            dst.delete()
            src.copyTo(dst, overwrite = true)
            logger.lifecycle("Fixed libc++_shared.so for $abi -> ${src.absolutePath}")
        }
    }
}

// Run after all rustBuild tasks but before JNI lib merging
afterEvaluate {
    tasks.matching { it.name.startsWith("merge") && it.name.contains("JniLibFolders") }.configureEach {
        dependsOn(fixLibcppShared)
    }
    tasks.matching { it.name.startsWith("rustBuild") }.configureEach {
        finalizedBy(fixLibcppShared)
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.6.1")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.8.0")
    implementation(platform("com.google.firebase:firebase-bom:33.1.0"))
    implementation("com.google.firebase:firebase-messaging-ktx")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("androidx.test:core:1.6.1")
    testImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")