package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Share URI parsing delegated to Rust `locus_core`. */
internal data class ShareUri(
    val scheme: String,
    val rawUserInfo: String?,
    val host: String,
    val port: Int,
    val rawPath: String,
    val rawQuery: String?,
    val rawFragment: String?,
) {
    fun query(): Map<String, String> = parseQuery(rawQuery)

    companion object {
        fun parse(raw: String): ShareUri {
            val obj = RustCore.callObject("shareUriParse", JSONObject().put("raw", raw))
            fun optNullStr(key: String): String? =
                if (!obj.has(key) || obj.isNull(key)) null else obj.getString(key)
            return ShareUri(
                scheme = obj.getString("scheme"),
                rawUserInfo = optNullStr("rawUserInfo"),
                host = obj.getString("host"),
                port = obj.getInt("port"),
                rawPath = obj.optString("rawPath", ""),
                rawQuery = optNullStr("rawQuery"),
                rawFragment = optNullStr("rawFragment"),
            )
        }

        fun decode(value: String): String =
            RustCore.callString("shareUriDecode", JSONObject().put("value", value))

        fun parseQuery(raw: String?): Map<String, String> = raw.orEmpty()
            .split('&')
            .filter { it.isNotEmpty() }
            .associate { part ->
                val eq = part.indexOf('=')
                if (eq < 0) decode(part) to "" else decode(part.substring(0, eq)) to decode(part.substring(eq + 1))
            }

        fun socksUserInfo(raw: String?): String =
            RustCore.callString("shareUriSocksUserInfo", JSONObject().put("raw", raw ?: JSONObject.NULL))
    }
}
