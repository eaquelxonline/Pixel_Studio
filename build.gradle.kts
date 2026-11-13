// Pixel Studio — root build.
// Runtime policy: no third-party runtime libraries. Kotlin's own stdlib is the only runtime
// dependency of the Kotlin modules; tests use kotlin-test (test scope only).

plugins {
    kotlin("jvm") version "2.4.21" apply false
    // Declared here (not only in the Android module) so AGP and the Kotlin plugin share one classloader.
    id("com.android.application") version "9.5.1" apply false
}

allprojects {
    group = "com.pixel.studio"
    version = "0.1.0"
}

/** Builds the Rust core (Native/) for the host platform (desktop app and JNI tests). */
val buildRustCore by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds Native (pixcore) in release mode for the host platform."
    workingDir = file("Native")
    commandLine("cargo", "build", "--release")
    inputs.files("Native/Core.rs", "Native/Cargo.toml")
    outputs.dir("Native/target/release")
}
