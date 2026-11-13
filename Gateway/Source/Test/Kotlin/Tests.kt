// Pixel Studio — JVM tests: JSON, JNI bridge (golden render), MCP/REST gateway end to end.

package com.pixel.studio.gateway

import java.io.File
import java.net.HttpURLConnection
import java.net.URI
import java.nio.file.Files
import java.security.MessageDigest
import java.util.Base64
import javax.imageio.ImageIO
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

// ═══ BridgeTest ══════════════════════════════════════════════════════════════════════════════════

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

    /** Same script + same engine ⇒ bit-identical frames on every platform (see Native/Golden.jsonl). */
    @Test
    fun goldenSceneMatchesRust() {
        val dir = File(System.getProperty("pixel.golden.dir"))
        val expected = File(dir, "Golden.fingerprint").readText().trim().lines()
        PixelEngine(32, 32).use { e ->
            File(dir, "Golden.jsonl").readLines().filter { it.isNotBlank() }.forEach {
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

// ═══ GatewayTest ═════════════════════════════════════════════════════════════════════════════════

class GatewayTest {
    private lateinit var engine: PixelEngine
    private lateinit var gateway: AgentGateway
    private lateinit var workspace: File
    private var approve = false
    private val token = "test-token-123"

    @BeforeTest
    fun setUp() {
        engine = PixelEngine(32, 32)
        workspace = Files.createTempDirectory("pixel-ws").toFile()
        gateway = AgentGateway(engine, GatewayConfig(port = 0, token = token), DirectoryWorkspace(workspace), Confirmer { _, _, _ -> approve }).start()
    }

    @AfterTest
    fun tearDown() {
        gateway.close()
        engine.close()
        workspace.deleteRecursively()
    }

    class Reply(val status: Int, val body: ByteArray, val headers: Map<String, String>) {
        val text get() = String(body, Charsets.UTF_8)
        val json: Map<String, Any?> get() = Json.parseObject(text)
    }

    private fun http(method: String, path: String, body: String? = null, headers: Map<String, String> = emptyMap(), auth: Boolean = true): Reply {
        val c = URI("http://127.0.0.1:${gateway.port}$path").toURL().openConnection() as HttpURLConnection
        c.requestMethod = method
        if (auth) c.setRequestProperty("Authorization", "Bearer $token")
        headers.forEach { (k, v) -> c.setRequestProperty(k, v) }
        if (body != null) {
            c.doOutput = true
            c.setRequestProperty("Content-Type", "application/json")
            c.outputStream.use { it.write(body.toByteArray()) }
        }
        val status = c.responseCode
        val stream = if (status >= 400) c.errorStream else c.inputStream
        val bytes = stream?.use { it.readBytes() } ?: ByteArray(0)
        val hs = c.headerFields.filterKeys { it != null }.mapKeys { it.key.lowercase() }.mapValues { it.value.joinToString(",") }
        return Reply(status, bytes, hs)
    }

    private fun rawStatus(extraHeaders: String, host: Boolean = true): Int {
        java.net.Socket("127.0.0.1", gateway.port).use { s ->
            val body = """{"jsonrpc":"2.0","id":1,"method":"ping"}"""
            val hostLine = if (host) "Host: 127.0.0.1:${gateway.port}\r\n" else ""
            val req = "POST /mcp HTTP/1.1\r\n$hostLine${extraHeaders}Content-Type: application/json\r\nContent-Length: ${body.length}\r\nConnection: close\r\n\r\n$body"
            s.getOutputStream().write(req.toByteArray())
            val statusLine = s.getInputStream().bufferedReader().readLine()
            return statusLine.split(' ')[1].toInt()
        }
    }

    private var nextId = 1

    private fun legacy(method: String, params: Map<String, Any?> = emptyMap(), session: String? = null): Reply {
        val msg = mapOf("jsonrpc" to "2.0", "id" to nextId++, "method" to method, "params" to params)
        val h = linkedMapOf("Accept" to "application/json, text/event-stream")
        if (session != null) { h["Mcp-Session-Id"] = session; h["MCP-Protocol-Version"] = "2025-11-25" }
        return http("POST", "/mcp", Json.write(msg), h)
    }

    private fun modern(method: String, params: Map<String, Any?> = emptyMap(), version: String = "2026-07-28", name: String? = null, headers: Boolean = true): Reply {
        val meta = mapOf(
            "io.modelcontextprotocol/protocolVersion" to version,
            "io.modelcontextprotocol/clientCapabilities" to emptyMap<String, Any>(),
            "io.modelcontextprotocol/clientInfo" to mapOf("name" to "test-agent", "version" to "1.0"),
        )
        val msg = mapOf("jsonrpc" to "2.0", "id" to "req-${nextId++}", "method" to method, "params" to params + ("_meta" to meta))
        val h = linkedMapOf("Accept" to "application/json, text/event-stream")
        if (headers) {
            h["MCP-Protocol-Version"] = version
            h["Mcp-Method"] = method
            if (name != null) h["Mcp-Name"] = name
        }
        return http("POST", "/mcp", Json.write(msg), h)
    }

    private fun callLegacy(tool: String, args: Map<String, Any?>): Map<String, Any?> {
        val r = legacy("tools/call", mapOf("name" to tool, "arguments" to args))
        assertEquals(200, r.status, r.text)
        return r.json.obj("result")!!
    }

    @Test
    fun securityChecks() {
        assertEquals(200, http("GET", "/health", auth = false).status)
        assertEquals(401, http("POST", "/mcp", "{}", auth = false).status)
        assertEquals(401, http("POST", "/mcp", "{}", headers = mapOf("Authorization" to "Bearer wrong"), auth = false).status)
        // HttpURLConnection refuses to send Origin, so use a raw socket like a browser would.
        assertEquals(403, rawStatus("Origin: https://evil.example\r\nAuthorization: Bearer $token\r\n"))
        assertEquals(403, rawStatus("Host: attacker.example:${gateway.port}\r\nAuthorization: Bearer $token\r\n", host = false))
        assertEquals(200, rawStatus("Origin: http://localhost:3000\r\nAuthorization: Bearer $token\r\n"))
        assertEquals(405, http("GET", "/mcp").status)
        val okOrigin = legacy("ping")
        assertEquals(200, okOrigin.status)
    }

    @Test
    fun legacyHandshakeToolsAndDrawing() {
        val init = legacy("initialize", mapOf("protocolVersion" to "2025-06-18", "capabilities" to emptyMap<String, Any>(), "clientInfo" to mapOf("name" to "Claude Code", "version" to "2")))
        assertEquals(200, init.status)
        val result = init.json.obj("result")!!
        assertEquals("2025-06-18", result.str("protocolVersion"))
        assertTrue(result.str("instructions")!!.contains("canvas_inspect"))
        val session = init.headers["mcp-session-id"]
        assertNotNull(session)
        val note = http("POST", "/mcp", """{"jsonrpc":"2.0","method":"notifications/initialized"}""", mapOf("Mcp-Session-Id" to session))
        assertEquals(202, note.status)

        val tools = legacy("tools/list", session = session).json.obj("result")!!.list("tools")!!
        assertEquals(ToolCatalog.tools.size, tools.size)

        val set = legacy("tools/call", mapOf("name" to "set_pixel", "arguments" to mapOf("x" to 12, "y" to 18, "color" to "#D94B42")), session)
        assertEquals(false, set.json.obj("result")!!["isError"])
        val get = callLegacy("get_pixel", mapOf("x" to 12, "y" to 18))
        assertEquals("#D94B42", get.obj("structuredContent")!!.obj("result")!!.str("color"))
        val history = callLegacy("history_list", emptyMap()).obj("structuredContent")!!.obj("result")!!.list("entries")!!
        assertEquals("mcp:Claude Code", (history.first() as Map<*, *>)["actor"])

        val snap = callLegacy("canvas_snapshot", emptyMap())
        val image = snap.list("content")!!.first() as Map<*, *>
        assertEquals("image", image["type"])
        val png = ImageIO.read(Base64.getDecoder().decode(image["data"] as String).inputStream())
        assertEquals(512, png.width)

        val unknown = legacy("nope/nope")
        assertEquals(-32601L, unknown.json.obj("error")!!.long("code"))
        val badTool = legacy("tools/call", mapOf("name" to "rm_rf"))
        assertEquals(-32602L, badTool.json.obj("error")!!.long("code"))
    }

    @Test
    fun modernStatelessProtocol() {
        val discover = modern("server/discover")
        assertEquals(200, discover.status, discover.text)
        val d = discover.json.obj("result")!!
        assertEquals("complete", d.str("resultType"))
        assertTrue("2026-07-28" in d.list("supportedVersions")!!)
        assertEquals("pixel-studio", d.obj("_meta")!!.obj("io.modelcontextprotocol/serverInfo")!!.str("name"))

        val list = modern("tools/list").json.obj("result")!!
        assertEquals("public", list.str("cacheScope"))
        assertTrue(list.long("ttlMs")!! > 0)

        val missingName = modern("tools/call", mapOf("name" to "set_pixel", "arguments" to mapOf("x" to 1, "y" to 1, "color" to "#fff")))
        assertEquals(400, missingName.status)
        assertEquals(-32020L, missingName.json.obj("error")!!.long("code"))

        val noHeaders = modern("tools/list", headers = false)
        assertEquals(400, noHeaders.status)

        val ok = modern("tools/call", mapOf("name" to "draw_line", "arguments" to mapOf("x0" to 0, "y0" to 0, "x1" to 31, "y1" to 31, "color" to "#00FF00")), name = "draw_line")
        assertEquals(200, ok.status, ok.text)
        assertEquals(false, ok.json.obj("result")!!["isError"])
        assertEquals("complete", ok.json.obj("result")!!.str("resultType"))
        val history = engine.execute("history_list").result.list("entries")!!.first() as Map<*, *>
        assertEquals("mcp:test-agent", history["actor"])

        val badVersion = modern("tools/list", version = "1900-01-01")
        assertEquals(400, badVersion.status)
        assertEquals(-32022L, badVersion.json.obj("error")!!.long("code"))

        assertEquals(404, modern("nope/nope").status)
        val res = modern("resources/read", mapOf("uri" to "pixelstudio://project/current"), name = "pixelstudio://project/current")
        assertEquals(0L, res.json.obj("result")!!.long("ttlMs"))
    }

    @Test
    fun humanAndAgentShareTheCanvasWithoutOverwrites() {
        val seen = callLegacy("canvas_inspect", emptyMap()).obj("structuredContent")!!.long("revision")!!
        // The human paints while the agent is "thinking".
        engine.execute("set_pixel", mapOf("x" to 5, "y" to 5, "color" to "#FFFFFF"))
        val stale = callLegacy("set_pixel", mapOf("x" to 5, "y" to 5, "color" to "#000000", "expectedRevision" to seen))
        assertEquals(true, stale["isError"])
        assertTrue((stale.list("content")!!.first() as Map<*, *>)["text"].toString().contains("conflict"))
        assertEquals("#FFFFFF", engine.execute("get_pixel", mapOf("x" to 5, "y" to 5)).result.str("color"))
        // Idempotent retry.
        callLegacy("frame_add", mapOf("operationId" to "agent-op-1"))
        callLegacy("frame_add", mapOf("operationId" to "agent-op-1"))
        assertEquals(2, engine.execute("canvas_inspect").result.list("frames")!!.size)
    }

    @Test
    fun destructiveToolsNeedApproval() {
        engine.execute("frame_add")
        val denied = callLegacy("frame_delete", mapOf("frame" to 1))
        assertEquals(true, denied["isError"])
        val sneaky = callLegacy("transaction", mapOf("commands" to listOf(mapOf("op" to "frame_delete", "frame" to 1))))
        assertEquals(true, sneaky["isError"])
        assertEquals(2, engine.execute("canvas_inspect").result.list("frames")!!.size)
        val rest = http("POST", "/api/v1/commands", """{"op":"layer_delete","layer":0}""")
        assertEquals(403, rest.status)
        approve = true
        assertEquals(false, callLegacy("frame_delete", mapOf("frame" to 1))["isError"])
        assertEquals(1, engine.execute("canvas_inspect").result.list("frames")!!.size)
    }

    @Test
    fun restApiAndFunctionCalling() {
        val status = http("GET", "/api/v1/status")
        assertEquals(200, status.status)
        assertEquals("pixcore", status.json.obj("result")!!.str("engine"))
        for (f in listOf("mcp", "openai", "anthropic", "gemini")) assertEquals(200, http("GET", "/api/v1/tools?format=$f").status)
        val openai = Json.parse(http("GET", "/api/v1/tools?format=openai").text) as List<*>
        assertEquals("function", (openai.first() as Map<*, *>)["type"])

        val call = http("POST", "/api/v1/tools/draw_rectangle", """{"x":0,"y":0,"width":4,"height":4,"color":"#123456","filled":true}""", mapOf("X-Pixel-Actor" to "gpt"))
        assertEquals(200, call.status, call.text)
        val cmd = http("POST", "/api/v1/commands", """{"commands":[{"op":"get_pixel","x":1,"y":1},{"op":"render_frame"}]}""")
        val responses = cmd.json.list("responses")!!
        assertEquals("#123456", ((responses[0] as Map<*, *>)["result"] as Map<*, *>)["color"])
        assertTrue((responses[1] as Map<*, *>).containsKey("binaryBase64"))

        val snap = http("GET", "/api/v1/snapshot?scale=2")
        assertEquals("image/png", snap.headers["content-type"])
        assertEquals(64, ImageIO.read(snap.body.inputStream()).width)
        val conflict = http("POST", "/api/v1/commands", """{"op":"set_pixel","x":0,"y":0,"color":"#fff","expectedRevision":0}""")
        assertEquals(409, conflict.status)
        assertEquals(200, http("GET", "/api/v1/frame/0").status)
        assertEquals(200, http("GET", "/api/v1/frame/0/pixels?width=8&height=2").status)
        assertEquals(404, http("GET", "/api/v1/nothing").status)
    }

    @Test
    fun exportsStayInsideTheWorkspace() {
        engine.execute("set_pixel", mapOf("x" to 0, "y" to 0, "color" to "#FF0000"))
        val r = callLegacy("project_export", mapOf("format" to "spritesheet", "name" to "../../etc/passwd"))
        assertEquals(false, r["isError"], Json.write(r))
        val exported = File(workspace, "Exports").listFiles()!!.map { it.name }.sorted()
        assertEquals(listOf("etc_passwd.json", "etc_passwd.png"), exported)
        val saved = callLegacy("project_save", mapOf("name" to "Knight"))
        assertEquals(false, saved["isError"])
        assertTrue(File(workspace, "Knight.pxs").readBytes().copyOfRange(0, 3).contentEquals("PXS".toByteArray()))
        assertEquals("untitled", DirectoryWorkspace.sanitize("../"))
    }

    @Test
    fun catalogueMatchesEngineAndCommittedSchemas() {
        val engineOps = engine.execute("status").result.list("operations")!!.toSet()
        val missing = ToolExecutor.engineOps() - engineOps
        assertTrue(missing.isEmpty(), "tools reference unknown engine ops: $missing")
        val dir = File(System.getProperty("pixel.protocol.dir"))
        for ((name, text) in schemaFiles()) {
            val committed = File(dir, name)
            assertTrue(committed.isFile && committed.readText() == text, "$name is stale: run ./gradlew :Gateway:exportToolSchemas")
        }
    }

    @Test
    fun stdioAdapterBridgesToTheApp() {
        val bridge = StdioBridge("http://127.0.0.1:${gateway.port}/mcp", token)
        val init = Json.parseObject(bridge.forward("""{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"stdio-client","version":"1"}}}""")!!)
        assertEquals("2025-11-25", init.obj("result")!!.str("protocolVersion"))
        assertNull(bridge.forward("""{"jsonrpc":"2.0","method":"notifications/initialized"}"""))
        val call = Json.parseObject(bridge.forward("""{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_pixel","arguments":{"x":3,"y":4,"color":"#ABCDEF"}}}""")!!)
        assertEquals(false, call.obj("result")!!["isError"])
        assertEquals("mcp:stdio-client", (engine.execute("history_list").result.list("entries")!!.first() as Map<*, *>)["actor"])
        val modernLine = """{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_pixel","arguments":{"x":3,"y":4},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"""
        val modernReply = Json.parseObject(bridge.forward(modernLine)!!)
        assertEquals("#ABCDEF", modernReply.obj("result")!!.obj("structuredContent")!!.obj("result")!!.str("color"))
        val offline = StdioBridge("http://127.0.0.1:1/mcp", token).forward("""{"jsonrpc":"2.0","id":9,"method":"ping"}""")!!
        assertTrue(offline.contains("not reachable"))
    }

    @Test
    fun threeDimensionalPixelArtOverMcp() {
        fun call(tool: String, args: Map<String, Any?>): Map<String, Any?> {
            val r = callLegacy(tool, args)
            assertEquals(false, r["isError"], "$tool failed: ${Json.write(r)}")
            return r
        }
        call("voxel_model_create", mapOf("name" to "Bot", "width" to 48, "height" to 64, "depth" to 48))
        call("texture_generate", mapOf("name" to "steel", "kind" to "brushed_metal", "size" to 32, "metallic" to 0.9))
        call("rig_bone_add", mapOf("name" to "hips", "head" to listOf(24, 30, 24), "tail" to listOf(24, 36, 24)))
        call("rig_bone_add", mapOf("name" to "upper_leg_l", "parent" to "hips", "head" to listOf(28, 30, 24), "tail" to listOf(28, 2, 24)))
        call("rig_bone_add", mapOf("name" to "upper_leg_r", "parent" to "hips", "head" to listOf(20, 30, 24), "tail" to listOf(20, 2, 24)))
        call("voxel_shape", mapOf("shape" to "box", "min" to listOf(17, 30, 19), "max" to listOf(31, 55, 29), "color" to "#8899AA", "material" to "steel", "bone" to "hips"))
        call("voxel_shape", mapOf("shape" to "capsule", "a" to listOf(28, 29, 24), "b" to listOf(28, 2, 24), "radius" to 3, "color" to "#334455", "bone" to "upper_leg_l", "mirror_x" to true))
        call("anim3d_generate", mapOf("kind" to "walk", "frames" to 8))
        val render = call("voxel_render", mapOf("yaw" to 45, "width" to 96, "animation" to "walk", "frame" to 2, "outline" to true))
        val image = render.list("content")!!.first() as Map<*, *>
        assertEquals("image", image["type"])
        assertEquals(96, ImageIO.read(Base64.getDecoder().decode(image["data"] as String).inputStream()).width)
        val normals = call("voxel_render_sheet", mapOf("animation" to "walk", "directions" to 8, "width" to 32, "pass" to "normal"))
        assertEquals(8L, normals.obj("structuredContent")!!.obj("result")!!.long("rows"))
        val baked = call("voxel_render_to_frames", mapOf("animation" to "walk", "directions" to 4, "width" to 64))
        assertEquals(32L, baked.obj("structuredContent")!!.obj("result")!!.long("framesAdded"))
        assertEquals(32, engine.execute("canvas_inspect").result.list("frames")!!.size)
        val inspect = call("voxel_inspect", emptyMap()).obj("structuredContent")!!.obj("result")!!
        assertEquals("Bot", (inspect.list("models")!!.first() as Map<*, *>)["name"])
        val deleted = callLegacy("voxel_model_delete", mapOf("model" to "Bot"))
        assertEquals(true, deleted["isError"], "deleting a model needs the human's approval")
    }

    @Test
    fun pixelArtCraftTools() {
        val drawn = callLegacy("draw_ascii", mapOf("x" to 2, "y" to 2, "rows" to listOf(".oo.", "osso", ".oo."), "legend" to mapOf("o" to "#1B1B2F", "s" to "#F6CDB0")))
        assertEquals(false, drawn["isError"], Json.write(drawn))
        val read = callLegacy("read_region", mapOf("x" to 2, "y" to 2, "width" to 4, "height" to 3)).obj("structuredContent")!!.obj("result")!!
        assertEquals(listOf(".00.", "0110", ".00."), read.list("rows"))
        assertEquals(false, callLegacy("outline", emptyMap())["isError"])
        assertEquals("#", engine.execute("get_pixel", mapOf("x" to 1, "y" to 3)).result.str("color")!!.take(1), "selective outline added")
        val ramp = callLegacy("color_ramp", mapOf("base" to "#3FA34D", "steps" to 5)).obj("structuredContent")!!.obj("result")!!.list("ramp")!!
        assertEquals(5, ramp.size)
        assertTrue(callLegacy("palette_list", emptyMap()).obj("structuredContent")!!.obj("result")!!.list("palettes")!!.size >= 10)
        assertEquals(false, callLegacy("palette_load", mapOf("name" to "endesga-32"))["isError"])
        assertEquals(false, callLegacy("quantize", mapOf("palette" to "swatches", "dither" to "bayer4"))["isError"])
        val normal = callLegacy("export_normal_map", mapOf("scale" to 2))
        assertEquals("image", (normal.list("content")!!.first() as Map<*, *>)["type"])
    }

    @Test
    fun gameProductionToolsWriteEngineFiles() {
        fun call(tool: String, args: Map<String, Any?>): Map<String, Any?> {
            val r = callLegacy(tool, args)
            assertEquals(false, r["isError"], "$tool failed: ${Json.write(r)}")
            return r
        }
        call("tileset_generate_blob", mapOf("name" to "moss", "tile_size" to 8, "fill" to "#3E7B3A", "lip" to 2))
        call("tilemap_create", mapOf("name" to "cave", "tileset" to "moss", "columns" to 6, "rows" to 4))
        call("tilemap_terrain", mapOf("tilemap" to "cave", "rows" to listOf("......", ".####.", "######", "######")))
        call("layer_create", mapOf("name" to "Ground"))
        call("tilemap_bake", mapOf("tilemap" to "cave", "layer" to "Ground"))
        call("slice_set", mapOf("name" to "hurtbox", "x" to 2, "y" to 2, "width" to 6, "height" to 8, "pivot" to listOf(3, 8)))
        call("layer_blend", mapOf("layer" to "Ground", "mode" to "multiply"))
        val lit = call("render_lit", mapOf("lights" to listOf(mapOf("x" to 10, "y" to 10, "color" to "#FFAA55")), "bands" to 4))
        assertEquals("image", (lit.list("content")!!.first() as Map<*, *>)["type"])
        val tiled = call("project_export", mapOf("format" to "tiled", "name" to "cave", "tilemap" to "cave"))
        val atlas = call("project_export", mapOf("format" to "atlas", "name" to "hero"))
        call("project_export", mapOf("format" to "godot", "name" to "hero_frames"))
        val files = File(workspace, "Exports").listFiles()!!.map { it.name }.toSet()
        assertTrue(files.containsAll(listOf("cave.tmj", "cave_tileset.png", "hero.json", "hero.png", "hero_frames.tres", "hero_frames.png")), files.toString())
        val tmj = Json.parseObject(File(workspace, "Exports/cave.tmj").readText())
        assertEquals("cave_tileset.png", (tmj.list("tilesets")!!.first() as Map<*, *>)["image"])
        val aseprite = Json.parseObject(File(workspace, "Exports/hero.json").readText())
        assertEquals("hero.png", aseprite.obj("meta")!!.str("image"))
        assertEquals("hurtbox", (aseprite.obj("meta")!!.list("slices")!!.first() as Map<*, *>)["name"])
        assertTrue(File(workspace, "Exports/hero_frames.tres").readText().contains("res://hero_frames.png"))
        assertTrue(Json.write(tiled).length < 4000 && Json.write(atlas).length < 200_000, "replies stay small")
        val gd = call("gamedev_inspect", emptyMap()).obj("structuredContent")!!.obj("result")!!
        assertEquals(listOf("......", ".####.", "######", "######"), (gd.list("tilemaps")!!.first() as Map<*, *>)["terrain"])
        assertEquals(true, callLegacy("tilemap_delete", mapOf("tilemap" to "cave"))["isError"], "deleting needs approval")
    }

    @Test
    fun characterArtFinishingTools() {
        fun call(tool: String, args: Map<String, Any?>): Map<String, Any?> {
            val r = callLegacy(tool, args)
            assertEquals(false, r["isError"], "$tool failed: ${Json.write(r)}")
            return r
        }
        call("draw_ellipse", mapOf("x" to 4, "y" to 6, "width" to 24, "height" to 22, "color" to "#FCE3D2", "filled" to true))
        call("layer_create", mapOf("name" to "Bangs"))
        call("draw_rectangle", mapOf("layer" to "Bangs", "x" to 4, "y" to 3, "width" to 24, "height" to 6, "color" to "#F7B6CF", "filled" to true))
        val shaded = call("auto_shade", mapOf("layer" to 0, "light" to "top-left", "cast_shadow" to 3, "rim" to "#8FE6FF"))
        assertTrue((shaded.obj("structuredContent")!!.obj("result")!!.long("changed") ?: 0) > 20)
        call("outline", mapOf("layer" to "Bangs", "mode" to "selout", "darkness" to 0.45))
        call("draw_sparkle", mapOf("points" to listOf(listOf(2, 2, 2), listOf(29, 29)), "layer" to "Bangs"))
        call("gradient_map", mapOf("colors" to listOf("#141C3A", "#8EA6D8", "#E6EEFF"), "scope" to "frame", "strength" to 0.4))
        call("pixel_cleanup", mapOf("layer" to "Bangs"))
    }
}
