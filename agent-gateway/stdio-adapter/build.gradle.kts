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
    testImplementation(kotlin("test"))
}

application {
    applicationName = "pixstudio-mcp"
    mainClass.set("com.pixel.studio.stdio.StdioAdapterKt")
}

tasks.test { useJUnitPlatform() }
