package com.pixel.studio.bridge

import java.io.File

/**
 * Raw JNI surface of the Rust core (`libpixcore.so` / `pixcore.dll`).
 *
 * The symbols are implemented by hand in `core-rust/src/ffi`. Prefer [PixelEngine], which
 * owns the handle lifecycle and decodes responses.
 */
object NativeBridge {
    /** Must equal `pixcore::ffi::ABI_VERSION`. */
    const val EXPECTED_ABI = 1

    @Volatile
    private var loaded = false

    /**
     * Loads the native library once. Resolution order:
     * 1. system property `pixcore.path` or env `PIXCORE_PATH` (absolute file path),
     * 2. `System.loadLibrary("pixcore")` (APK `lib/<abi>/`, or `java.library.path` on desktop).
     */
    @Synchronized
    fun ensureLoaded() {
        if (loaded) return
        val explicit = System.getProperty("pixcore.path") ?: System.getenv("PIXCORE_PATH")
        if (explicit != null && File(explicit).isFile) {
            System.load(File(explicit).absolutePath)
        } else {
            System.loadLibrary("pixcore")
        }
        val abi = abiVersion()
        check(abi == EXPECTED_ABI) { "pixcore ABI mismatch: native=$abi expected=$EXPECTED_ABI" }
        loaded = true
    }

    @JvmStatic external fun abiVersion(): Int

    /** Returns an opaque handle, or 0 if the size is invalid. */
    @JvmStatic external fun create(width: Int, height: Int): Long

    @JvmStatic external fun destroy(handle: Long)

    /** Current revision, or -1 for an invalid handle. Cheap; safe to poll from UI timers. */
    @JvmStatic external fun revision(handle: Long): Long

    /** Framed response: `u32 LE json length | UTF-8 JSON | binary`. */
    @JvmStatic external fun call(handle: Long, request: ByteArray, payload: ByteArray?): ByteArray?
}
