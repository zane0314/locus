package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Hysteria2 hop_interval duration parser/validator delegated to Rust `locus_core`. */
internal fun hysteriaHopInterval(raw: String): String =
    RustCore.callString("hysteriaHopInterval", JSONObject().put("raw", raw))
