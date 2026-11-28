// The Android app's logic that does not need Android: server API, session,
// sync. The Kotlin bindings of the Rust core are generated into
// build/generated/uniffi from the same Rust sources the app ships.

plugins {
    kotlin("jvm")
    kotlin("plugin.serialization")
}

// Java 17 bytecode, so the Android app can use this module as it is.
java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

val repoRoot = rootProject.projectDir.parentFile
val cargoTarget = File(repoRoot, "target")
val generated = layout.buildDirectory.dir("generated/uniffi")
val hostLib = File(cargoTarget, "debug/" + System.mapLibraryName("amanfold_ffi"))

// Debug build of the core for the host: generates the bindings and runs the
// tests. The app builds its own release libraries per Android ABI.
val cargoHost by tasks.registering(Exec::class) {
    workingDir = repoRoot
    commandLine("cargo", "build", "-p", "amanfold-ffi", "-p", "amanfold-server")
    inputs.dir(File(repoRoot, "crates"))
    inputs.file(File(repoRoot, "Cargo.lock"))
    outputs.file(hostLib)
}

val generateBindings by tasks.registering(Exec::class) {
    dependsOn(cargoHost)
    workingDir = repoRoot
    val out = generated.get().asFile
    commandLine(
        "cargo", "run", "-q", "-p", "uniffi-bindgen", "--", "generate", "--no-format",
        "--library", hostLib.path, "--language", "kotlin", "--out-dir", out.path,
    )
    inputs.file(hostLib)
    outputs.dir(out)
}

sourceSets.main {
    kotlin.srcDir(generateBindings.map { generated.get() })
}

dependencies {
    api("net.java.dev.jna:jna:5.17.0")
    api("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    testImplementation(kotlin("test"))
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
}

tasks.test {
    useJUnitPlatform()
    dependsOn(cargoHost)
    systemProperty("jna.library.path", File(cargoTarget, "debug").path)
    systemProperty("amanfold.server", File(cargoTarget, "debug/amanfold-server").path)
    testLogging { events("passed", "failed"); exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL }
}
