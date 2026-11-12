package com.pixel.studio.json

/**
 * Minimal, dependency-free JSON for Pixel Studio.
 *
 * Parsed values map to plain Kotlin types:
 * object → [LinkedHashMap] (insertion order kept), array → [List], string → [String],
 * integer → [Long], other numbers → [Double], `true`/`false` → [Boolean], `null` → `null`.
 */
object Json {
    private const val MAX_DEPTH = 128

    class ParseException(message: String) : IllegalArgumentException(message)

    /** Already-serialized JSON that should be embedded verbatim when writing. */
    class Raw(val json: String)

    fun parse(text: String): Any? {
        val p = Parser(text)
        p.skipWs()
        val v = p.value(0)
        p.skipWs()
        if (p.pos != text.length) throw ParseException("unexpected trailing data at ${p.pos}")
        return v
    }

    @Suppress("UNCHECKED_CAST")
    fun parseObject(text: String): Map<String, Any?> =
        parse(text) as? Map<String, Any?> ?: throw ParseException("expected a JSON object")

    fun write(value: Any?): String = StringBuilder(64).also { writeTo(value, it) }.toString()

    fun writeTo(value: Any?, out: StringBuilder) {
        when (value) {
            null -> out.append("null")
            is Raw -> out.append(value.json)
            is String -> writeString(value, out)
            is Boolean -> out.append(if (value) "true" else "false")
            is Int, is Long, is Short, is Byte -> out.append(value.toString())
            is Double -> writeDouble(value, out)
            is Float -> writeDouble(value.toDouble(), out)
            is Number -> writeDouble(value.toDouble(), out)
            is Map<*, *> -> {
                out.append('{')
                var first = true
                for ((k, v) in value) {
                    if (!first) out.append(',')
                    first = false
                    writeString(k.toString(), out)
                    out.append(':')
                    writeTo(v, out)
                }
                out.append('}')
            }
            is Iterable<*> -> {
                out.append('[')
                var first = true
                for (v in value) {
                    if (!first) out.append(',')
                    first = false
                    writeTo(v, out)
                }
                out.append(']')
            }
            is Array<*> -> writeTo(value.asList(), out)
            is IntArray -> writeTo(value.asList(), out)
            is LongArray -> writeTo(value.asList(), out)
            is Enum<*> -> writeString(value.name, out)
            else -> writeString(value.toString(), out)
        }
    }

    private fun writeDouble(d: Double, out: StringBuilder) {
        when {
            d.isNaN() || d.isInfinite() -> out.append("null")
            d == Math.rint(d) && Math.abs(d) < 9.007199254740992E15 -> out.append(d.toLong())
            else -> out.append(d.toString())
        }
    }

    private fun writeString(s: String, out: StringBuilder) {
        out.append('"')
        for (c in s) {
            when (c) {
                '"' -> out.append("\\\"")
                '\\' -> out.append("\\\\")
                '\n' -> out.append("\\n")
                '\r' -> out.append("\\r")
                '\t' -> out.append("\\t")
                ' ' -> out.append("\\u2028")
                ' ' -> out.append("\\u2029")
                else -> if (c < ' ') out.append(String.format("\\u%04x", c.code)) else out.append(c)
            }
        }
        out.append('"')
    }

    private class Parser(val s: String) {
        var pos = 0

        fun skipWs() {
            while (pos < s.length && (s[pos] == ' ' || s[pos] == '\t' || s[pos] == '\n' || s[pos] == '\r')) pos++
        }

        fun fail(msg: String): Nothing = throw ParseException("$msg at $pos")

        fun value(depth: Int): Any? {
            if (depth > MAX_DEPTH) fail("nesting too deep")
            if (pos >= s.length) fail("unexpected end of input")
            return when (s[pos]) {
                'n' -> literal("null", null)
                't' -> literal("true", true)
                'f' -> literal("false", false)
                '"' -> string()
                '[' -> array(depth)
                '{' -> obj(depth)
                '-', in '0'..'9' -> number()
                else -> fail("unexpected character '${s[pos]}'")
            }
        }

        fun literal(lit: String, v: Any?): Any? {
            if (!s.startsWith(lit, pos)) fail("invalid literal")
            pos += lit.length
            return v
        }

        fun array(depth: Int): List<Any?> {
            pos++
            val out = ArrayList<Any?>()
            skipWs()
            if (pos < s.length && s[pos] == ']') { pos++; return out }
            while (true) {
                skipWs()
                out.add(value(depth + 1))
                skipWs()
                if (pos >= s.length) fail("unterminated array")
                when (s[pos++]) {
                    ',' -> continue
                    ']' -> return out
                    else -> fail("expected ',' or ']'")
                }
            }
        }

        fun obj(depth: Int): Map<String, Any?> {
            pos++
            val out = LinkedHashMap<String, Any?>()
            skipWs()
            if (pos < s.length && s[pos] == '}') { pos++; return out }
            while (true) {
                skipWs()
                if (pos >= s.length || s[pos] != '"') fail("expected object key")
                val key = string()
                skipWs()
                if (pos >= s.length || s[pos] != ':') fail("expected ':'")
                pos++
                skipWs()
                out[key] = value(depth + 1)
                skipWs()
                if (pos >= s.length) fail("unterminated object")
                when (s[pos++]) {
                    ',' -> continue
                    '}' -> return out
                    else -> fail("expected ',' or '}'")
                }
            }
        }

        fun number(): Any {
            val start = pos
            if (s[pos] == '-') pos++
            val digits = pos
            while (pos < s.length && s[pos].isAsciiDigit()) pos++
            if (pos == digits) fail("invalid number")
            var integral = true
            if (pos < s.length && s[pos] == '.') {
                integral = false
                pos++
                val f = pos
                while (pos < s.length && s[pos].isAsciiDigit()) pos++
                if (pos == f) fail("invalid fraction")
            }
            if (pos < s.length && (s[pos] == 'e' || s[pos] == 'E')) {
                integral = false
                pos++
                if (pos < s.length && (s[pos] == '+' || s[pos] == '-')) pos++
                val e = pos
                while (pos < s.length && s[pos].isAsciiDigit()) pos++
                if (pos == e) fail("invalid exponent")
            }
            val text = s.substring(start, pos)
            if (integral) text.toLongOrNull()?.let { return it }
            return text.toDouble()
        }

        private fun Char.isAsciiDigit() = this in '0'..'9'

        fun string(): String {
            pos++
            val sb = StringBuilder()
            while (true) {
                if (pos >= s.length) fail("unterminated string")
                val c = s[pos++]
                when {
                    c == '"' -> return sb.toString()
                    c == '\\' -> {
                        if (pos >= s.length) fail("truncated escape")
                        when (val e = s[pos++]) {
                            '"' -> sb.append('"')
                            '\\' -> sb.append('\\')
                            '/' -> sb.append('/')
                            'b' -> sb.append('\b')
                            'f' -> sb.append('\u000C')
                            'n' -> sb.append('\n')
                            'r' -> sb.append('\r')
                            't' -> sb.append('\t')
                            'u' -> {
                                if (pos + 4 > s.length) fail("truncated \\u escape")
                                val code = s.substring(pos, pos + 4).toIntOrNull(16) ?: fail("invalid \\u escape")
                                pos += 4
                                sb.append(code.toChar())
                            }
                            else -> fail("invalid escape '\\$e'")
                        }
                    }
                    c < ' ' -> fail("control character in string")
                    else -> sb.append(c)
                }
            }
        }
    }
}

// ---- Typed accessors for parsed objects ----------------------------------------------------

@Suppress("UNCHECKED_CAST")
fun Map<String, Any?>.obj(key: String): Map<String, Any?>? = this[key] as? Map<String, Any?>

@Suppress("UNCHECKED_CAST")
fun Map<String, Any?>.list(key: String): List<Any?>? = this[key] as? List<Any?>

fun Map<String, Any?>.str(key: String): String? = this[key] as? String

fun Map<String, Any?>.long(key: String): Long? = when (val v = this[key]) {
    is Long -> v
    is Int -> v.toLong()
    is Double -> if (v == Math.rint(v)) v.toLong() else null
    else -> null
}

fun Map<String, Any?>.bool(key: String): Boolean? = this[key] as? Boolean

/** Small builder: `jsonObject("a" to 1, "b" to listOf(2))`. */
fun jsonObject(vararg pairs: Pair<String, Any?>): LinkedHashMap<String, Any?> = linkedMapOf(*pairs)
