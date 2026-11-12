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
    api(project(":app-kotlin:shared"))
    testImplementation(kotlin("test"))
    testImplementation(project(":agent-gateway:stdio-adapter"))
}

val nativeLib = rootProject.file("core-rust/target/release/" + System.mapLibraryName("pixcore"))

application {
    // Headless server: engine + MCP/REST gateway without a UI (for CI and server use).
    mainClass.set("com.pixel.studio.gateway.HeadlessServerKt")
    applicationDefaultJvmArgs = listOf("-Dpixcore.path=${nativeLib.absolutePath}")
}

tasks.named<JavaExec>("run") {
    dependsOn(rootProject.tasks.named("buildRustCore"))
}

// Regenerates the JSON files in protocol/tools from the Kotlin tool catalogue.
tasks.register<JavaExec>("exportToolSchemas") {
    group = "build"
    description = "Writes the shared tool schemas (MCP / OpenAI / Anthropic / Gemini) to protocol/tools."
    classpath = sourceSets.main.get().runtimeClasspath
    mainClass.set("com.pixel.studio.gateway.tools.ToolSchemaExportKt")
    args(rootProject.file("protocol/tools").absolutePath)
}

tasks.test {
    useJUnitPlatform()
    dependsOn(rootProject.tasks.named("buildRustCore"))
    systemProperty("pixcore.path", nativeLib.absolutePath)
    systemProperty("pixel.protocol.dir", rootProject.file("protocol/tools").absolutePath)
}
