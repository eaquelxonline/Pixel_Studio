package com.pixel.studio.gateway

import com.pixel.studio.bridge.PixelEngine
import com.pixel.studio.gateway.tools.Confirmer
import com.pixel.studio.gateway.tools.DirectoryWorkspace
import java.io.File

/**
 * Runs the engine and the agent gateway without a UI:
 * `--size 64x64 --port 38471 --workspace ./workspace [--token TOKEN] [--allow-destructive]`
 */
fun main(args: Array<String>) {
    fun opt(name: String) = args.indexOf(name).takeIf { it >= 0 && it + 1 < args.size }?.let { args[it + 1] }
    val (w, h) = (opt("--size") ?: "32x32").lowercase().split('x').map { it.toInt() }
    val port = opt("--port")?.toInt() ?: GatewayConfig.DEFAULT_PORT
    val workspace = File(opt("--workspace") ?: "workspace").absoluteDir()
    val confirmer = if ("--allow-destructive" in args) Confirmer.APPROVE_ALL else Confirmer.DENY_ALL
    val engine = PixelEngine(w, h)
    val config = opt("--token")?.let { GatewayConfig(port = port, token = it) } ?: GatewayConfig(port = port)
    val gateway = AgentGateway(engine, config, DirectoryWorkspace(workspace), confirmer).start()
    println("Pixel Studio headless gateway — canvas ${w}x$h, workspace ${workspace.path}")
    println("MCP endpoint: ${gateway.mcpUrl}")
    gateway.connectionSnippets().forEach { (k, v) -> println("\n[$k]\n$v") }
    Runtime.getRuntime().addShutdownHook(Thread { gateway.close(); engine.close() })
    Thread.currentThread().join()
}

private fun File.absoluteDir(): File = absoluteFile.also { it.mkdirs() }
