import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm")
    application
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    implementation(project(":app-kotlin:shared"))
    implementation(project(":agent-gateway"))
    testImplementation(kotlin("test"))
}

val nativeLib = rootProject.file("core-rust/target/release/" + System.mapLibraryName("pixcore"))

application {
    applicationName = "PixelStudio"
    mainClass.set("com.pixel.studio.desktop.PixelStudioAppKt")
}

// Bundle the hero demo script so "Load hero demo" works offline.
tasks.processResources {
    from(rootProject.file("assets/knight/knight.jsonl")) { into("com/pixel/studio/desktop") }
}

tasks.named<JavaExec>("run") {
    dependsOn(rootProject.tasks.named("buildRustCore"))
    jvmArgs("-Dpixcore.path=${nativeLib.absolutePath}", "-Dsun.java2d.uiScale.enabled=true")
}

tasks.test {
    useJUnitPlatform()
    dependsOn(rootProject.tasks.named("buildRustCore"))
    systemProperty("pixcore.path", nativeLib.absolutePath)
    systemProperty("java.awt.headless", "true")
}

/**
 * Builds a self-contained Windows installer with the JDK's jpackage (run on Windows x64,
 * after `cargo build --release --target x86_64-pc-windows-msvc`). The JRE is bundled, so
 * users need no Java installation. Use -PpackageType=app-image for a portable folder.
 */
tasks.register<Exec>("packageWindows") {
    group = "distribution"
    description = "Creates PixelStudio (.exe installer or app-image) with jpackage, bundling pixcore.dll and a JRE."
    dependsOn("installDist")
    val type = (project.findProperty("packageType") as String?) ?: "exe"
    val dll = rootProject.file("core-rust/target/x86_64-pc-windows-msvc/release/pixcore.dll")
    val input = layout.buildDirectory.dir("install/PixelStudio/lib")
    val out = layout.buildDirectory.dir("jpackage")
    doFirst {
        check(dll.isFile) { "Build the core first: cargo build --release --target x86_64-pc-windows-msvc (in core-rust)" }
        dll.copyTo(input.get().file("pixcore.dll").asFile, overwrite = true)
    }
    commandLine(
        "jpackage", "--type", type,
        "--name", "PixelStudio", "--app-version", project.version.toString(),
        "--vendor", "Pixel Studio", "--description", "Pixel Studio — AI artist workspace",
        "--input", input.get().asFile.absolutePath,
        "--main-jar", "windows-${project.version}.jar",
        "--main-class", "com.pixel.studio.desktop.PixelStudioAppKt",
        "--icon", rootProject.file("branding/pixel-studio.ico").absolutePath,
        "--java-options", "-Djava.library.path=\$APPDIR",
        "--add-modules", "java.base,java.desktop,java.logging",
        "--win-menu", "--win-shortcut", "--win-dir-chooser",
        "--file-associations", rootProject.file("app-kotlin/windows/packaging/pxs-association.properties").absolutePath,
        "--dest", out.get().asFile.absolutePath,
    )
}
