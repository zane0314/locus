package com.zane.zanebox.backup

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Guard size, nesting depth, and single-object structure in Rust `locus_core`. */
internal fun backupJson(text: String): JSONObject {
    RustCore.callBool("validateBackupJson", JSONObject().put("text", text))
    return JSONObject(text)
}

internal fun backupJson(bytes: ByteArray): JSONObject =
    backupJson(Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(bytes)).toString())
