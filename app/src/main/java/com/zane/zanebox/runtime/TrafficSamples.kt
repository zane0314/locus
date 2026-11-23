package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

internal data class TrafficSample(val id: String, val app: String, val domain: String, val tx: Long, val rx: Long)

internal object TrafficSamples {
    const val MAX_BYTES = 4 * 1024 * 1024

    fun domain(value: String): String =
        RustCore.callString("trafficDomain", JSONObject().put("value", value))

    fun parse(text: String): List<TrafficSample> {
        val arr = RustCore.callArray("parseTrafficSamples", JSONObject().put("text", text))
        return (0 until arr.length()).map { i ->
            val o = arr.getJSONObject(i)
            TrafficSample(
                id = o.getString("id"),
                app = o.getString("app"),
                domain = o.getString("domain"),
                tx = o.getLong("tx"),
                rx = o.getLong("rx"),
            )
        }
    }
}
