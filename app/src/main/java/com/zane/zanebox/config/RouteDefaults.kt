package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.RouteRule
import org.json.JSONObject

/** NekoBox ProfileManager defaults; broad country rules run after app policies. Delegated to Rust `locus_core`. */
internal fun factoryRouteRules(country: String): List<RouteRule> {
    val arr = RustCore.callArray("factoryRouteRules", JSONObject().put("country", country))
    return (0 until arr.length()).map { i ->
        val r = arr.getJSONObject(i)
        RouteRule(
            id = r.getLong("id"),
            name = r.getString("name"),
            domains = r.optString("domains", ""),
            packages = r.optString("packages", ""),
            ipCidrs = r.optString("ipCidrs", ""),
            outbound = r.optString("outbound", "proxy"),
            enabled = r.optBoolean("enabled", true),
            order = r.optInt("order", 0),
            advanced = r.optString("advanced", "{}"),
            prioritize = r.optBoolean("prioritize", false),
        )
    }
}

internal fun withFactoryRouteDefaults(data: AppData, country: String): AppData {
    if (data.setting("factoryRouteDefaultsVersion") == "1") return data
    val raw = RustCore.callString(
        "withFactoryRouteDefaults",
        JSONObject().put("data", data.toRustJson()).put("country", country),
    )
    return AppData.fromRustJson(raw)
}
