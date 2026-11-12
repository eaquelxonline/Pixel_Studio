package com.pixel.studio

import com.pixel.studio.bridge.NativeBridge
import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.json.Json
import com.pixel.studio.json.list
import com.pixel.studio.json.long
import com.pixel.studio.json.obj
import com.pixel.studio.json.str
import java.io.File
import java.security.MessageDigest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class JsonTest {
    @Test
    fun roundTrip() {
        val text = """{"a":1,"b":-2.5,"c":"x\"y\\z\né","d":[true,false,null,{}],"e":12345678901234}"""
        val v = Json.parseObject(text)
        assertEquals(1L, v["a"])
        assertEquals(-2.5, v["b"])
        assertEquals("x\"y\\z\né", v["c"])
        assertEquals(12345678901234L, v.long("e"))
        assertEquals(v, Json.parseObject(Json.write(v)))
    }

    @Test
    fun rejectsGarbage() {
        for (bad in listOf("", "{", "[1,]", "{\"a\" 1}", "tru", "[1] 2", "\"abc")) {
            assertTrue(runCatching { Json.parse(bad) }.isFailure, "should reject: $bad")
        }
    }
}

class BridgeTest {
    @Test
    fun abiAndLifecycle() {
        NativeBridge.ensureLoaded()
        assertEquals(NativeBridge.EXPECTED_ABI, NativeBridge.abiVersion())
        val h = NativeBridge.create(8, 8)
        assertTrue(h != 0L)
        assertEquals(0L, NativeBridge.revision(h))
        NativeBridge.destroy(h)
        assertEquals(-1L, NativeBridge.revision(h), "destroyed handle must be rejected")
        val stale = PixelEngine.decode(NativeBridge.call(h, "{\"op\":\"status\"}".toByteArray(), null)!!)
        assertEquals("invalid_handle", stale.errorCode)
        assertEquals(0L, NativeBridge.create(0, 10), "invalid size must not create an engine")
    }

    @Test
    fun pixelRoundTripThroughJni() {
        PixelEngine(32, 32).use { e ->
            var notified = -1L
            e.addChangeListener { notified = it }
            val set = e.execute("set_pixel", mapOf("x" to 12, "y" to 18, "color" to "#D94B42"), actor = "kotlin-test")
            assertTrue(set.ok, set.raw)
            assertEquals(1L, notified)
            val get = e.execute("get_pixel", mapOf("x" to 12, "y" to 18))
            assertEquals("#D94B42", get.result.str("color"))
            val raw = e.execute("render_frame")
            assertEquals(32 * 32 * 4, raw.binary.size)
            val i = (18 * 32 + 12) * 4
            assertEquals(0xD9, raw.binary[i].toInt() and 0xFF)
            assertEquals(255, raw.binary[i + 3].toInt() and 0xFF)
        }
    }

    @Test
    fun conflictAndBinaryPayload() {
        PixelEngine(16, 16).use { e ->
            e.execute("set_pixel", mapOf("x" to 1, "y" to 1, "color" to "#FFFFFF"), expectedRevision = 0)
            val stale = e.execute("set_pixel", mapOf("x" to 1, "y" to 1, "color" to "#000000"), expectedRevision = 0)
            assertFalse(stale.ok)
            assertEquals("conflict", stale.errorCode)
            val saved = e.execute("project_save")
            assertTrue(saved.binary.size > 20)
            PixelEngine(1, 1).use { other ->
                val loaded = other.execute("project_load", payload = saved.binary)
                assertTrue(loaded.ok, loaded.raw)
                assertContentEquals(e.execute("render_frame").binary, other.execute("render_frame").binary)
            }
        }
    }

    /** Same script + same engine ⇒ bit-identical frames on every platform (see tests/golden). */
    @Test
    fun goldenSceneMatchesRust() {
        val dir = File(System.getProperty("pixel.golden.dir"))
        val expected = File(dir, "scene.fingerprint").readText().trim().lines()
        PixelEngine(32, 32).use { e ->
            File(dir, "scene.jsonl").readLines().filter { it.isNotBlank() }.forEach {
                val r = e.executeRaw(it.toByteArray())
                assertTrue(r.ok, "golden step failed: $it -> ${r.raw}")
            }
            val frames = e.execute("canvas_inspect").result.list("frames")!!.size
            val prints = (0 until frames).map { f ->
                val img = e.execute("render_frame", mapOf("frame" to f))
                val w = img.result.long("width")!!.toInt()
                val h = img.result.long("height")!!.toInt()
                fnv1a(w, h, img.binary)
            }
            assertEquals(expected, prints)
        }
    }

    @Test
    fun pngExportDecodesWithPlatformImageIo() {
        PixelEngine(8, 8).use { e ->
            e.execute("draw_rectangle", mapOf("x" to 0, "y" to 0, "width" to 8, "height" to 8, "color" to "#3366FF", "filled" to true))
            val png = e.execute("export_png", mapOf("scale" to 3)).binary
            val img = javax.imageio.ImageIO.read(png.inputStream())
            assertEquals(24, img.width)
            assertEquals(0xFF3366FF.toInt(), img.getRGB(5, 5))
            val gif = e.execute("export_gif").binary
            assertEquals(8, javax.imageio.ImageIO.read(gif.inputStream()).width)
            val digest = MessageDigest.getInstance("SHA-256").digest(png)
            assertEquals(32, digest.size)
        }
    }

    @Test
    fun statusListsOperations() {
        PixelEngine(4, 4).use { e ->
            val ops = e.execute("status").result.list("operations")!!
            assertTrue("set_pixel" in ops && "transaction" in ops)
            assertEquals("pixcore", e.execute("status").result.str("engine"))
            assertTrue(e.execute("canvas_inspect").result.obj("tags") == null)
        }
    }

    private fun fnv1a(w: Int, h: Int, pixels: ByteArray): String {
        var hash = -0x340d631b7bdddcdbL // 0xcbf29ce484222325
        val prime = 0x100000001b3L
        fun eat(b: Int) { hash = (hash xor (b.toLong() and 0xFF)) * prime }
        for (v in intArrayOf(w, h)) for (k in 0 until 4) eat(v ushr (8 * k))
        for (b in pixels) eat(b.toInt())
        return String.format("%016x", hash)
    }
}
