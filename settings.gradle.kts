// Pixel Studio — Gradle settings.
pluginManagement {
    repositories {
        gradlePluginPortal()
        google()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "PixelStudio"

include(":Gateway", ":Windows")

// The Android module needs the Android SDK. It is included only when an SDK is configured
// (ANDROID_HOME / ANDROID_SDK_ROOT or sdk.dir in local.properties) so desktop-only machines
// and CI runners without the SDK can still build and test everything else.
val localProps = file("local.properties")
val sdkFromProps = if (localProps.exists()) {
    localProps.readLines().firstOrNull { it.startsWith("sdk.dir=") }?.substringAfter("=")
} else null
val androidSdk = sdkFromProps ?: System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
if (androidSdk != null && file(androidSdk).isDirectory) {
    include(":Android")
}
