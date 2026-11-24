package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.NodeStatusUpdate
import org.json.JSONArray
import org.json.JSONObject

/** The sing-box group a routing target resolves to; null means the target only reports latency. */
internal data class TargetGroup(val tag: String, val urlTest: Boolean, val mergeId: Long = 0)

internal const val GROUP_SELECTION_PREFIX = "groupSelection."
internal const val GROUP_SELECTION_DELAY_PREFIX = "groupSelectionDelay."

internal fun savedChoiceDelay(data: AppData, tag: String, nodePing: Int): Int =
    RustCore.callInt(
        "savedChoiceDelay",
        JSONObject().put("data", data.toJson()).put("tag", tag).put("nodePing", nodePing),
    )

internal fun restorableSelectorChoices(data: AppData, config: String): Map<String, String> {
    val obj = RustCore.callObject(
        "restorableSelectorChoices",
        JSONObject().put("data", data.toJson()).put("config", config),
    )
    return obj.keys().asSequence().associateWith { obj.getString(it) }
}

internal fun expandLatencySyncUpdates(
    data: AppData,
    valid: List<NodeStatusUpdate>,
    excludedIds: Set<Long>,
): List<NodeStatusUpdate> {
    if (valid.isEmpty()) return valid
    val obj = RustCore.callObject(
        "latencySyncSiblings",
        JSONObject()
            .put("data", data.toRustJson())
            .put("testedIds", JSONArray(valid.map { it.id }))
            .put("excludedIds", JSONArray(excludedIds.toList())),
    )
    if (obj.length() == 0) return valid
    val byId = data.nodes.associateBy { it.id }
    val extras = LinkedHashMap<Long, NodeStatusUpdate>()
    valid.forEach { update ->
        val arr = obj.optJSONArray(update.id.toString()) ?: return@forEach
        for (i in 0 until arr.length()) {
            val sid = arr.getLong(i)
            val node = byId[sid] ?: continue
            extras[sid] = NodeStatusUpdate(sid, node.outbound, update.ping, update.status)
        }
    }
    return if (extras.isEmpty()) valid else valid + extras.values
}

internal class TargetPlan private constructor(
    val target: String,
    val serviceKey: String,
    val members: List<Long>,
    val group: TargetGroup?,
    private val outbounds: Map<Long, String>,
    private val fingerprint: String,
) {
    fun winner(latest: AppData, pings: Map<Long, Int>): Pair<Long, Int>? {
        val current = latest.nodes.associate { it.id to it.outbound }
        return members.filter { id -> (pings[id] ?: 0) > 0 && current[id] == outbounds[id] }
            .minByOrNull { pings.getValue(it) }?.let { it to pings.getValue(it) }
    }

    fun nativeResults(latest: AppData, pings: Map<Long, Int>): Map<String, Int> {
        val current = latest.nodes.associate { it.id to it.outbound }
        return members.associate { id -> "node-$id" to (pings[id]?.takeIf { it > 0 && current[id] == outbounds[id] } ?: 0) }
    }

    fun sameAs(other: TargetPlan): Boolean =
        other.members.toSet() == members.toSet() && other.group == group && other.fingerprint == fingerprint

    fun stillValid(latest: AppData): Boolean =
        create(latest, target, serviceKey)?.let(::sameAs) == true

    companion object {
        private fun fromJson(obj: JSONObject): TargetPlan {
            val memArr = obj.getJSONArray("members")
            val members = (0 until memArr.length()).map { memArr.getLong(it) }
            val grpObj = obj.optJSONObject("group")
            val group = grpObj?.let {
                TargetGroup(it.getString("tag"), it.getBoolean("urlTest"), it.getLong("mergeId"))
            }
            val outObj = obj.getJSONObject("outbounds")
            val outbounds = outObj.keys().asSequence().associate { it.toLong() to outObj.getString(it) }
            return TargetPlan(
                target = obj.getString("target"),
                serviceKey = obj.getString("serviceKey"),
                members = members,
                group = group,
                outbounds = outbounds,
                fingerprint = obj.getString("fingerprint"),
            )
        }

        fun create(data: AppData, target: String, serviceKey: String): TargetPlan? {
            val obj = RustCore.callNullableObject(
                "targetPlanCreate",
                JSONObject()
                    .put("data", data.toRustJson())
                    .put("target", target)
                    .put("serviceKey", serviceKey),
            ) ?: return null
            return fromJson(obj)
        }

        fun createBatch(data: AppData, targets: List<String>, serviceKey: String = ""): Map<String, TargetPlan> {
            if (targets.isEmpty()) return emptyMap()
            val obj = RustCore.callObject(
                "targetPlanCreateBatch",
                JSONObject()
                    .put("data", data.toRustJson())
                    .put("targets", org.json.JSONArray(targets))
                    .put("serviceKey", serviceKey),
            )
            return obj.keys().asSequence().associateWith { fromJson(obj.getJSONObject(it)) }
        }

        fun resolveGroup(data: AppData, target: String, serviceKey: String): TargetGroup? =
            create(data, target, serviceKey)?.group
    }
}
