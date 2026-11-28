// Amanfold for Android.
//
//   :client  Plain Kotlin (JVM): the server API, the account and vault
//            session, the sync loop. Wraps the generated Kotlin bindings of
//            the Rust security core. Tested on the JVM against a real
//            amanfold-server, so it also runs where no Android SDK is.
//   :app     The Android app: Compose UI, biometric unlock, Autofill Service.
//            Included when an Android SDK is configured.

pluginManagement {
    repositories {
        // Google's repository only when building the app (it hosts the
        // Android plugin and AndroidX); the client needs Maven Central only.
        if (System.getenv("ANDROID_HOME") != null || System.getenv("ANDROID_SDK_ROOT") != null) google()
        mavenCentral()
        gradlePluginPortal()
    }
    plugins {
        id("com.android.application") version "8.11.1"
        kotlin("android") version "2.2.21"
        kotlin("jvm") version "2.2.21"
        kotlin("plugin.serialization") version "2.2.21"
        kotlin("plugin.compose") version "2.2.21"
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        if (System.getenv("ANDROID_HOME") != null || System.getenv("ANDROID_SDK_ROOT") != null) google()
        mavenCentral()
    }
}

rootProject.name = "amanfold-android"
include(":client")

if (System.getenv("ANDROID_HOME") != null || System.getenv("ANDROID_SDK_ROOT") != null) include(":app")
