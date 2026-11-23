package com.zane.zanebox.data

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Canonical SHA-256 node identity computed in Rust `locus_core`. */
object NodeIdentity {
    fun key(outbound: String): String =
        RustCore.callString("nodeIdentityKey", JSONObject().put("outbound", outbound))
}
