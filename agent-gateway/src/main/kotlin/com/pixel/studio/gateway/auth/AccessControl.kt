package com.pixel.studio.gateway.auth

import com.pixel.studio.gateway.http.HttpRequest
import java.net.URI
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64

/**
 * Bearer-token authentication plus Origin/Host validation (DNS-rebinding protection).
 *
 * Tokens are 256-bit random values generated on the device; they never leave it unless the
 * user copies the connection snippet into an agent's configuration.
 */
class AccessControl(
    token: String = generateToken(),
    /** Extra allowed browser origins, e.g. "https://my-tool.example". Localhost is always allowed. */
    private val allowedOrigins: Set<String> = emptySet(),
    /** When serving only on loopback, also require a loopback Host header. */
    private val loopbackOnly: Boolean = true,
) {
    @Volatile var token: String = token
        private set

    fun rotateToken(): String = generateToken().also { token = it }

    sealed class Verdict {
        object Allowed : Verdict()
        class Denied(val status: Int, val reason: String) : Verdict()
    }

    fun check(request: HttpRequest): Verdict {
        val origin = request.header("origin")
        if (origin != null && origin != "null" && !isAllowedOrigin(origin)) {
            return Verdict.Denied(403, "origin not allowed")
        }
        if (origin == "null") return Verdict.Denied(403, "opaque origin not allowed")
        if (loopbackOnly) {
            val host = request.header("host")?.let { hostOnly(it) }
            if (host != null && host !in LOOPBACK_NAMES) return Verdict.Denied(403, "host not allowed")
        }
        val auth = request.header("authorization") ?: return Verdict.Denied(401, "missing bearer token")
        if (!auth.startsWith("Bearer ", ignoreCase = true)) return Verdict.Denied(401, "expected a bearer token")
        val presented = auth.substring(7).trim()
        val ok = MessageDigest.isEqual(presented.toByteArray(Charsets.UTF_8), token.toByteArray(Charsets.UTF_8))
        return if (ok) Verdict.Allowed else Verdict.Denied(401, "invalid token")
    }

    private fun isAllowedOrigin(origin: String): Boolean {
        if (origin in allowedOrigins) return true
        val host = runCatching { URI(origin).host }.getOrNull() ?: return false
        return host.removePrefix("[").removeSuffix("]") in LOOPBACK_NAMES
    }

    private fun hostOnly(hostHeader: String): String {
        val h = hostHeader.trim()
        if (h.startsWith("[")) return h.substring(1, h.indexOf(']').takeIf { it > 0 } ?: h.length)
        return h.substringBefore(':')
    }

    companion object {
        val LOOPBACK_NAMES = setOf("localhost", "127.0.0.1", "::1")

        fun generateToken(): String {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            return "pxs_" + Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
        }
    }
}
