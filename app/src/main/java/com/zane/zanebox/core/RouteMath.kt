package com.zane.zanebox.core

import org.json.JSONArray
import org.json.JSONObject

/** Delegates CIDR prefix subtraction to Rust `locus_core`. */
internal object RouteMath {
    fun subtract(include: List<String>, exclude: List<String>): List<String> {
        val arr = RustCore.callArray(
            "routeMathSubtract",
            JSONObject()
                .put("base", JSONArray(include))
                .put("excluded", JSONArray(exclude)),
        )
        return (0 until arr.length()).map { arr.getString(it) }
    }
}
