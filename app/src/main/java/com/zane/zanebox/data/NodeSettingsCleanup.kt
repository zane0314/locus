package com.zane.zanebox.data

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Prunes stale node/selector settings in Rust `locus_core` without re-validating untouched records. */
fun AppData.withoutStaleNodeSettings(): AppData {
    val json = RustCore.callString("withoutStaleNodeSettings", JSONObject().put("data", toRustJson()))
    return AppData.fromRustJson(json, validateResult = false)
}
