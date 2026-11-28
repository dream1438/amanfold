// The Amanfold Android app. The Rust core is built per ABI with cargo-ndk
// (task cargoAndroid) and packaged as libamanfold_ffi.so; the Kotlin bindings
// and the session logic come from :client.

plugins {
    id("com.android.application")
    kotlin("android")
    kotlin("plugin.compose")
}

val repoRoot = rootProject.projectDir.parentFile
val jniOut = layout.buildDirectory.dir("rustJniLibs")

android {
    namespace = "com.amanfold.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.amanfold.app"
        minSdk = 28
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0-beta"
        // The production server is not live yet; see docs/SERVER.md.
        buildConfigField("String", "DEFAULT_SERVER", "\"https://api.amanfold.com\"")
        buildConfigField("boolean", "ALLOW_LOCAL_HTTP", "false")
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            // The emulator reaches a local amanfold-server here.
            buildConfigField("String", "DEFAULT_SERVER", "\"http://10.0.2.2:8080\"")
            buildConfigField("boolean", "ALLOW_LOCAL_HTTP", "true")
        }
        release {
            // Off for the beta: UniFFI and JNA need careful keep rules first.
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets["main"].jniLibs.srcDir(jniOut)

    packaging {
        resources.excludes += setOf("META-INF/AL2.0", "META-INF/LGPL2.1", "META-INF/LICENSE*")
    }

    lint {
        abortOnError = true
        warningsAsErrors = false
        disable += setOf("MissingTranslation")
    }
}

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

// libamanfold_ffi.so for phones (arm64) and the emulator (x86_64). Needs
// cargo-ndk and the Rust Android targets; CI installs both. Pass
// -Pamanfold.abis=arm64-v8a,x86_64,armeabi-v7a to change the set.
val cargoAndroid by tasks.registering(Exec::class) {
    val abis = (findProperty("amanfold.abis") as String? ?: "arm64-v8a,x86_64").split(",")
    workingDir = repoRoot
    val args = mutableListOf("cargo", "ndk", "--platform", "28", "-o", jniOut.get().asFile.path)
    abis.forEach { args += listOf("-t", it.trim()) }
    args += listOf("build", "-p", "amanfold-ffi", "--release")
    commandLine(args)
    inputs.dir(File(repoRoot, "crates"))
    inputs.file(File(repoRoot, "Cargo.lock"))
    outputs.dir(jniOut)
}
tasks.named("preBuild") { dependsOn(cargoAndroid) }

dependencies {
    implementation(project(":client")) {
        // The JVM build of JNA; Android needs the AAR with its own natives.
        exclude(group = "net.java.dev.jna", module = "jna")
    }
    implementation("net.java.dev.jna:jna:5.17.0@aar")

    val composeBom = platform("androidx.compose:compose-bom:2025.06.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")

    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.1")
    implementation("androidx.lifecycle:lifecycle-process:2.9.1")
    implementation("androidx.biometric:biometric:1.1.0")
}
