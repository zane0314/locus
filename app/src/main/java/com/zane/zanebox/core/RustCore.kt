package com.zane.zanebox.core

import org.json.JSONArray
import org.json.JSONObject
import java.io.File

/**
 * Single JNI bridge into `liblocus_core` (macOS `.dylib` in JVM unit tests, Android `arm64-v8a` `.so` on device).
 * Every operation uses panic-safe `catch_unwind` on the Rust side and returns `{"ok": ...}` or `{"err": "..."}`.
 */
object RustCore {
    init {
        loadLibrary()
    }

    private fun loadLibrary() {
        try {
            System.loadLibrary("locus_core")
            return
        } catch (first: UnsatisfiedLinkError) {
            val candidates = buildList {
                System.getProperty("locus.rust.lib")?.let { add(File(it)) }
                val cwd = File(System.getProperty("user.dir") ?: ".")
                val names = listOf("liblocus_core.dylib", "liblocus_core.so")
                for (base in listOf(cwd, cwd.parentFile).filterNotNull()) {
                    for (name in names) {
                        add(File(base, ".build/rust-host/$name"))
                        add(File(base, "rust/.build/host/$name"))
                        add(File(base, "rust/locus_core/target/release/$name"))
                        add(File(base, "rust/locus_core/target/debug/$name"))
                    }
                }
            }
            for (file in candidates) {
                if (file.isFile) {
                    System.load(file.absolutePath)
                    return
                }
            }
            throw first
        }
    }

    @JvmStatic
    private external fun nativeInvoke(op: String, payload: String): String

    @JvmStatic
    private external fun nativeGeoAssetCode(kind: String, bytes: ByteArray): String

    private fun unwrap(raw: String): Any? {
        val root = JSONObject(raw)
        if (root.has("err")) {
            val msg = root.getString("err")
            if (msg.startsWith("STATE:")) {
                throw IllegalStateException(msg.removePrefix("STATE:"))
            }
            throw IllegalArgumentException(msg)
        }
        val value = root.opt("ok")
        return if (value === JSONObject.NULL) null else value
    }

    fun call(op: String, payload: JSONObject = JSONObject()): Any? =
        unwrap(nativeInvoke(op, payload.toString()))

    fun callString(op: String, payload: JSONObject = JSONObject()): String =
        call(op, payload) as String

    fun callNullableString(op: String, payload: JSONObject = JSONObject()): String? =
        call(op, payload) as? String

    fun callBool(op: String, payload: JSONObject = JSONObject()): Boolean =
        call(op, payload) as Boolean

    fun callLong(op: String, payload: JSONObject = JSONObject()): Long =
        (call(op, payload) as Number).toLong()

    fun callInt(op: String, payload: JSONObject = JSONObject()): Int =
        (call(op, payload) as Number).toInt()

    fun callObject(op: String, payload: JSONObject = JSONObject()): JSONObject =
        call(op, payload) as JSONObject

    fun callNullableObject(op: String, payload: JSONObject = JSONObject()): JSONObject? =
        call(op, payload) as? JSONObject

    fun callArray(op: String, payload: JSONObject = JSONObject()): JSONArray =
        call(op, payload) as JSONArray

    fun geoAssetCode(kind: String, bytes: ByteArray): String =
        unwrap(nativeGeoAssetCode(kind, bytes)) as String

    fun version(): String {
        val info = callObject("version")
        return "${info.optString("name", "locus_core")} ${info.optString("version", "0.1.0")}"
    }
}
