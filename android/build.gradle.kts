// Plugin versions are in settings.gradle.kts. The Kotlin plugin is loaded
// once here so :client and :app share it.
plugins {
    kotlin("jvm") apply false
    kotlin("android") apply false
    kotlin("plugin.serialization") apply false
    kotlin("plugin.compose") apply false
}
