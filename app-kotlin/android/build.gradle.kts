// Pixel Studio — Android edition (platform Views only; no AndroidX, no Compose).
// AGP 9 compiles Kotlin itself ("built-in Kotlin"), using the Kotlin version from the root build.

plugins {
    id("com.android.application")
}

android {
    namespace = "com.pixel.studio"
    compileSdk {
        version = release(37) { minorApiLevel = 2 }
    }
    ndkVersion = "30.0.16248370"

    defaultConfig {
        applicationId = "com.pixel.studio"
        minSdk = 30
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories += layout.buildDirectory.dir("rustJniLibs").get().asFile.path
            assets.directories += layout.buildDirectory.dir("demoAssets").get().asFile.path
        }
    }
}

dependencies {
    implementation(project(":app-kotlin:shared"))
    implementation(project(":agent-gateway"))
}

// ---- Rust core for Android ABIs --------------------------------------------------------
// Builds core-rust with the NDK's clang as linker and stages libpixcore.so per ABI.

val ndkDir = androidComponents.sdkComponents.ndkDirectory
val rustAbis = mapOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android")
val hostTag = when {
    System.getProperty("os.name").startsWith("Windows") -> "windows-x86_64"
    System.getProperty("os.name").startsWith("Mac") -> "darwin-x86_64"
    else -> "linux-x86_64"
}

val rustTasks = rustAbis.map { (abi, triple) ->
    tasks.register<Exec>("cargoBuild-$abi") {
        group = "build"
        description = "Builds pixcore for $triple"
        workingDir = rootProject.file("core-rust")
        inputs.dir(rootProject.file("core-rust/src"))
        outputs.file(rootProject.file("core-rust/target/$triple/release/libpixcore.so"))
        doFirst {
            val bin = ndkDir.get().asFile.resolve("toolchains/llvm/prebuilt/$hostTag/bin")
            val clang = bin.resolve("$triple${30}-clang" + if (hostTag.startsWith("windows")) ".cmd" else "")
            environment("CARGO_TARGET_${triple.uppercase().replace('-', '_')}_LINKER", clang.absolutePath)
            environment("CC_${triple.replace('-', '_')}", clang.absolutePath)
            environment("AR_${triple.replace('-', '_')}", bin.resolve("llvm-ar").absolutePath)
        }
        commandLine("cargo", "build", "--release", "--target", triple)
    }
}

val stageRustLibs = tasks.register<Copy>("stageRustLibs") {
    dependsOn(rustTasks)
    rustAbis.forEach { (abi, triple) ->
        from(rootProject.file("core-rust/target/$triple/release/libpixcore.so")) { into(abi) }
    }
    into(layout.buildDirectory.dir("rustJniLibs"))
}

val stageDemoAssets = tasks.register<Copy>("stageDemoAssets") {
    from(rootProject.file("assets/knight/knight.jsonl"))
    into(layout.buildDirectory.dir("demoAssets"))
}

tasks.named("preBuild") { dependsOn(stageRustLibs, stageDemoAssets) }
