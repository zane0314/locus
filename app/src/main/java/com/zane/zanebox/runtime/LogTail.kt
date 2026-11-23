package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import org.json.JSONObject
import java.io.File

private const val LOG_TAIL_MAX_BYTES = 256 * 1024

/** Reads only the final bounded byte window via Rust `locus_core`, dropping incomplete UTF-8 and partial first lines. */
internal object LogTail {
    fun read(file: File, maxBytes: Int = LOG_TAIL_MAX_BYTES): List<String> {
        val arr = RustCore.callArray(
            "readLogTail",
            JSONObject().put("path", file.absolutePath).put("maxBytes", maxBytes),
        )
        return (0 until arr.length()).map { arr.getString(it) }
    }
}
