package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject

internal data class RulePreview(val title: String, val subtitle: String, val count: Int?, val status: String, val sample: List<String>)

internal fun buildRulePreviews(
    data: AppData,
    geoPresent: (String) -> Boolean = { false },
    builtinText: (String) -> String = { "" },
    installed: Set<String>? = null,
): List<RulePreview> {
    val keys = smartPolicyKeys(data)
    val builtinObj = JSONObject()
    keys.forEach { key ->
        if (data.setting("smartUrl.$key").isBlank()) {
            val txt = builtinText(key)
            if (txt.isNotEmpty()) builtinObj.put(key, txt)
        }
    }
    val req = JSONObject()
        .put("data", data.toJson())
        .put("geoipPresent", geoPresent("geoip"))
        .put("geositePresent", geoPresent("geosite"))
        .put("builtinTexts", builtinObj)
    if (installed != null) req.put("installed", JSONArray(installed.toList()))
    val arr = RustCore.callArray("buildRulePreviews", req)
    return (0 until arr.length()).map { i ->
        val item = arr.getJSONObject(i)
        val count = if (!item.has("count") || item.isNull("count")) null else item.getInt("count")
        val sampleArr = item.getJSONArray("sample")
        val sample = (0 until sampleArr.length()).map { sampleArr.getString(it) }
        RulePreview(
            title = item.getString("title"),
            subtitle = item.getString("subtitle"),
            count = count,
            status = item.getString("status"),
            sample = sample,
        )
    }
}
