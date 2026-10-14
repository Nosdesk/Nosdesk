import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Release signing. CI (or a local release build) writes app/keystore.properties
// from a base64 secret; it and the keystore are gitignored. Without it, release
// builds stay unsigned (debug builds are always debug-signed).
val keystorePropertiesFile = file("keystore.properties")
val keystoreProperties = Properties().apply {
    if (keystorePropertiesFile.exists()) {
        keystorePropertiesFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 36
    namespace = "com.nosdesk.app"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.nosdesk.app"
        minSdk = 24
        targetSdk = 36
        versionCode = (System.getenv("NOSDESK_VERSION_CODE")
            ?: tauriProperties.getProperty("tauri.android.versionCode", "1")).toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        create("release") {
            if (keystorePropertiesFile.exists()) {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            applicationIdSuffix = ".debug"
            // The debug variant installs alongside the Play build under its own
            // package, so both wore the same name and icon in the launcher. That
            // cost real time: a layout fix was twice judged "still broken" from
            // the release app. A placeholder rather than resValue, because
            // values/strings.xml already defines app_name and defining it twice
            // in one variant is a duplicate-resource build error.
            manifestPlaceholders["appLabel"] = "Nosdesk debug"
            // Its own OIDC callback scheme. With both packages declaring an
            // unqualified `nosdesk://`, Android delivered the redirect to
            // whichever app held the "open by default" preference, so signing
            // in to one completed in the other. src-tauri/src/lib.rs returns the
            // matching value to the JS that builds the redirect_uri.
            manifestPlaceholders["oidcScheme"] = "nosdesk.debug"
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
            // Keeps the shipped label coming from values/strings.xml; only the
            // debug variant overrides it.
            manifestPlaceholders["appLabel"] = "@string/app_name"
            manifestPlaceholders["oidcScheme"] = "nosdesk"
            if (keystorePropertiesFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
            isMinifyEnabled = true
            // The Rust core is native code: bundle its symbol table so Play
            // symbolicates native crashes and ANRs (no separate upload).
            ndk {
                debugSymbolLevel = "SYMBOL_TABLE"
            }
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")

// FCM: reads app/google-services.json. Applied last so it sees the android
// config above. Requires google-services.json in this module (gen/android/app/).
apply(plugin = "com.google.gms.google-services")