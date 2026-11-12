package com.pixel.studio.gateway.jsonrpc

/** JSON-RPC 2.0 helpers and the error codes used by MCP. */
object JsonRpc {
    const val PARSE_ERROR = -32700
    const val INVALID_REQUEST = -32600
    const val METHOD_NOT_FOUND = -32601
    const val INVALID_PARAMS = -32602
    const val INTERNAL_ERROR = -32603

    // MCP 2026-07-28 reserved codes.
    const val HEADER_MISMATCH = -32020
    const val MISSING_CLIENT_CAPABILITY = -32021
    const val UNSUPPORTED_PROTOCOL_VERSION = -32022

    fun result(id: Any?, result: Any?): Map<String, Any?> = linkedMapOf("jsonrpc" to "2.0", "id" to id, "result" to result)

    fun error(id: Any?, code: Int, message: String, data: Any? = null): Map<String, Any?> {
        val err = linkedMapOf<String, Any?>("code" to code, "message" to message)
        if (data != null) err["data"] = data
        return linkedMapOf("jsonrpc" to "2.0", "id" to id, "error" to err)
    }

    /** Valid ids are strings or integers (null only appears in error responses). */
    fun isValidId(id: Any?): Boolean = id is String || id is Long || id is Int
}

/** Thrown by handlers to produce a JSON-RPC error response. */
class RpcException(val code: Int, message: String, val data: Any? = null, val httpStatus: Int = 200) : Exception(message)
