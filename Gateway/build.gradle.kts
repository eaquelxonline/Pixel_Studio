// Pixel Studio — shared JVM core: JNI bridge, JSON and the agent gateway (MCP + REST).
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
    sourceSets {
        named("main") { kotlin.setSrcDirs(listOf("Source/Main/Kotlin")) }
        named("test") { kotlin.setSrcDirs(listOf("Source/Test/Kotlin")) }
    }
}

sourceSets {
    named("main") {
        java.setSrcDirs(emptyList<String>())
        resources.setSrcDirs(emptyList<String>())
    }
    named("test") {
        java.setSrcDirs(emptyList<String>())
        resources.setSrcDirs(emptyList<String>())
    }
}

dependencies {
    testImplementation(kotlin("test"))
}

val nativeLib = rootProject.file("Native/target/release/" + System.mapLibraryName("pixcore"))
val schemasDir = layout.projectDirectory.dir("Schemas").asFile

application {
    // Headless server: engine + MCP/REST gateway without a UI (for CI and server use).
    mainClass.set("com.pixel.studio.gateway.CoreKt")
    applicationDefaultJvmArgs = listOf("-Dpixcore.path=${nativeLib.absolutePath}")
}

// `pixstudio-mcp`: the STDIO adapter, shipped in the same distribution.
val stdioStartScripts = tasks.register<CreateStartScripts>("stdioStartScripts") {
    applicationName = "pixstudio-mcp"
    mainClass.set("com.pixel.studio.gateway.StdioKt")
    outputDir = layout.buildDirectory.dir("stdioScripts").get().asFile
    classpath = files(tasks.jar) + configurations.runtimeClasspath.get()
}
distributions { main { contents { from(stdioStartScripts) { into("bin") } } } }

tasks.named<JavaExec>("run") {
    dependsOn(rootProject.tasks.named("buildRustCore"))
}

// Regenerates Gateway/Schemas from the Kotlin tool catalogue.
tasks.register<JavaExec>("exportToolSchemas") {
    group = "build"
    description = "Writes the tool schemas (MCP / OpenAI / Anthropic / Gemini) to Gateway/Schemas."
    classpath = sourceSets.main.get().runtimeClasspath
    mainClass.set("com.pixel.studio.gateway.CoreKt")
    args("--export-schemas", schemasDir.absolutePath)
}

tasks.test {
    useJUnitPlatform()
    dependsOn(rootProject.tasks.named("buildRustCore"))
    systemProperty("pixcore.path", nativeLib.absolutePath)
    systemProperty("pixel.protocol.dir", schemasDir.absolutePath)
    systemProperty("pixel.golden.dir", rootProject.file("Native").absolutePath)
}
