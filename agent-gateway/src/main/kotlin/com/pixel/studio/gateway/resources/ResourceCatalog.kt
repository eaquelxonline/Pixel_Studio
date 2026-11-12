package com.pixel.studio.gateway.resources

import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.jsonrpc.JsonRpc
import com.pixel.studio.gateway.jsonrpc.RpcException
import com.pixel.studio.gateway.prompts.Guidance
import com.pixel.studio.json.Json
import java.util.Base64

/** Read-only context an agent can pull in: project state, palette, current frame, timing, guide. */
class ResourceCatalog(private val engine: PixelEngine) {
    private class Res(val uri: String, val name: String, val title: String, val description: String, val mimeType: String)

    private val resources = listOf(
        Res("pixelstudio://project/current", "project", "Current project", "Canvas size, layers, frames, tags and revision.", "application/json"),
        Res("pixelstudio://project/palette", "palette", "Palette", "Palette colors with indices.", "application/json"),
        Res("pixelstudio://frame/current", "frame", "Current frame image", "PNG of the active frame (all visible layers).", "image/png"),
        Res("pixelstudio://animation/current", "animation", "Animation timing", "Playback order, durations and FPS of all frames.", "application/json"),
        Res("pixelstudio://guide/pixel-art", "guide", "Pixel art guide", "Craft rules for clean pixel art and animation.", "text/markdown"),
    )

    fun list(): List<Map<String, Any?>> = resources.map {
        linkedMapOf("uri" to it.uri, "name" to it.name, "title" to it.title, "description" to it.description, "mimeType" to it.mimeType)
    }

    fun read(uri: String): Map<String, Any?> {
        val res = resources.firstOrNull { it.uri == uri } ?: throw RpcException(JsonRpc.INVALID_PARAMS, "resource not found: $uri", mapOf("uri" to uri))
        val content: Map<String, Any?> = when (res.name) {
            "project" -> textContent(res, Json.write(engine.execute("canvas_inspect").result))
            "palette" -> textContent(res, Json.write(engine.execute("palette_get").result))
            "animation" -> textContent(res, Json.write(engine.execute("animation_info").result))
            "guide" -> textContent(res, Guidance.PIXEL_ART_GUIDE)
            else -> {
                val png = engine.execute("canvas_snapshot").binary
                linkedMapOf("uri" to res.uri, "mimeType" to res.mimeType, "blob" to Base64.getEncoder().encodeToString(png))
            }
        }
        return linkedMapOf("contents" to listOf(content))
    }

    private fun textContent(res: Res, text: String) = linkedMapOf<String, Any?>("uri" to res.uri, "mimeType" to res.mimeType, "text" to text)
}
