package com.zane.zanebox.ui

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject

internal data class SmartPolicyRowSummary(
    val target: String,
    val lowestNode: String?,
    val latencyLabel: String?,
    val lowestPing: Int = -1,
)

internal data class SmartPanelSummary(
    val policyOrder: List<String>,
    val autoNodeCount: Int,
    val policies: Map<String, SmartPolicyRowSummary>,
)

/** Minimal raw-string AppData snapshot carrying only fields read by Rust `smart_panel_summary`. */
private fun smartSummaryDataJson(data: AppData): JSONObject {
    val nodes = JSONArray()
    data.nodes.forEach { n ->
        nodes.put(
            JSONObject()
                .put("id", n.id)
                .put("groupId", n.groupId)
                .put("name", n.name)
                .put("outbound", n.outbound)
                .put("ping", n.ping)
                .put("status", n.status),
        )
    }
    val groups = JSONArray()
    data.groups.forEach { g ->
        groups.put(
            JSONObject()
                .put("id", g.id)
                .put("name", g.name)
                .put("subscriptionUrl", g.subscriptionUrl)
                .put("enabled", g.enabled),
        )
    }
    val merges = JSONArray()
    data.merges.forEach { m ->
        merges.put(
            JSONObject()
                .put("id", m.id)
                .put("name", m.name)
                .put("nodeIds", JSONArray(m.nodeIds))
                .put("groupIds", JSONArray(m.groupIds)),
        )
    }
    val settings = JSONObject()
    data.settings.forEach { (k, v) ->
        when {
            k == "selectedNodeId" || k == "smartPolicyOrder" || k.startsWith("smart.") ||
                (k.startsWith("smartCustom.") && k.endsWith(".name")) -> settings.put(k, v)
            k == "smartRules.custom" -> settings.put(k, "")
        }
    }
    return JSONObject()
        .put("format", 1)
        .put("nodes", nodes)
        .put("groups", groups)
        .put("merges", merges)
        .put("settings", settings)
}

internal fun smartPanelSummary(data: AppData, testing: Set<Long>): SmartPanelSummary {
    val result = RustCore.callObject(
        "smartPanelSummary",
        JSONObject()
            .put("data", smartSummaryDataJson(data))
            .put("testing", JSONArray(testing.toList())),
    )
    val orderArr = result.optJSONArray("policyOrder") ?: JSONArray()
    val policyOrder = (0 until orderArr.length()).map { orderArr.getString(it) }
    val policiesArr = result.optJSONArray("policies") ?: JSONArray()
    val policies = LinkedHashMap<String, SmartPolicyRowSummary>(policiesArr.length())
    for (i in 0 until policiesArr.length()) {
        val item = policiesArr.getJSONObject(i)
        val lowest = if (item.isNull("lowestNode")) null else item.optString("lowestNode").takeIf { it.isNotEmpty() }
        val latency = if (item.isNull("latencyLabel")) null else item.optString("latencyLabel").takeIf { it.isNotEmpty() }
        policies[item.getString("key")] = SmartPolicyRowSummary(
            target = item.getString("target"),
            lowestNode = lowest,
            latencyLabel = latency,
            lowestPing = item.optInt("lowestPing", -1),
        )
    }
    return SmartPanelSummary(
        policyOrder = policyOrder,
        autoNodeCount = result.optInt("autoNodeCount", 0),
        policies = policies,
    )
}

internal fun smartLowestNode(data: AppData, value: String, testing: Set<Long>): String? =
    RustCore.callNullableString(
        "smartLowestNode",
        JSONObject()
            .put("data", data.toJson())
            .put("value", value)
            .put("testing", JSONArray(testing.toList())),
    )

/** The bare value: a group's lowest successful latency, or a status word (测试中/未测试/失败); no prefix of any kind. */
internal fun smartLatencyLabel(data: AppData, value: String, testing: Set<Long>): String? =
    targetLatencyLabel(data, value, testing)
