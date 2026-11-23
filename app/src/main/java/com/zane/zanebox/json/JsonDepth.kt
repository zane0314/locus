package com.zane.zanebox.json

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Delegates JSON nesting guard (<=128 depth) to Rust `locus_core`. */
internal fun requireJsonDepth(text: String) {
    RustCore.callBool("requireJsonDepth", JSONObject().put("text", text))
}
