import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm")
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    testImplementation(kotlin("test"))
}

val nativeLib = rootProject.file("core-rust/target/release/" + System.mapLibraryName("pixcore"))

tasks.test {
    useJUnitPlatform()
    dependsOn(rootProject.tasks.named("buildRustCore"))
    systemProperty("pixcore.path", nativeLib.absolutePath)
    systemProperty("pixel.golden.dir", rootProject.file("tests/golden").absolutePath)
}
