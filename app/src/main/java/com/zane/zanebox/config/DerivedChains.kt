package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.subscription.toRustJson
import org.json.JSONArray
import org.json.JSONObject

fun derivedGroupSourceId(group: Group): Long = JSONObject(group.options).optLong("chainSourceGroupId")
fun derivedNodeSourceId(node: Node): Long = JSONObject(node.metadata).optLong("chainSourceNodeId")

private fun AppData.largestRecordId(): Long =
    (nodes.map { it.id } + groups.map { it.id } + rules.map { it.id } + merges.map { it.id }).maxOrNull() ?: 0

private fun AppData.derivedIdAllocator(): () -> Long {
    var next = largestRecordId()
    return { ++next }
}

private fun remapNewIds(original: AppData, updated: AppData, allocateId: () -> Long): AppData {
    val oldGroupIds = original.groups.map { it.id }.toSet()
    val oldNodeIds = original.nodes.map { it.id }.toSet()
    val groupMap = updated.groups.filter { it.id !in oldGroupIds }.associate { it.id to allocateId() }
    val nodeMap = updated.nodes.filter { it.id !in oldNodeIds }.associate { it.id to allocateId() }
    if (groupMap.all { (k, v) -> k == v } && nodeMap.all { (k, v) -> k == v }) return updated
    fun mapTarget(v: String): String = when {
        v.startsWith("node:") -> nodeMap[v.substringAfter(':').toLongOrNull()]?.let { "node:$it" } ?: v
        v.startsWith("group:") -> groupMap[v.substringAfter(':').toLongOrNull()]?.let { "group:$it" } ?: v
        else -> v
    }
    val remappedGroups = updated.groups.map { g ->
        val ngId = groupMap[g.id] ?: g.id
        val opts = JSONObject(g.options)
        opts.optJSONObject("chain")?.let { c ->
            if (c.has("front")) c.put("front", mapTarget(c.getString("front")))
            if (c.has("landing")) c.put("landing", mapTarget(c.getString("landing")))
        }
        g.copy(
            id = ngId,
            frontProxy = nodeMap[g.frontProxy] ?: g.frontProxy,
            landingProxy = nodeMap[g.landingProxy] ?: g.landingProxy,
            options = opts.toString(),
        )
    }
    val remappedNodes = updated.nodes.map { n ->
        val nnId = nodeMap[n.id] ?: n.id
        val ngId = groupMap[n.groupId] ?: n.groupId
        val out = JSONObject(n.outbound)
        val nextOut = if (out.optString("type") == "chain" && out.has("node_ids")) {
            val arr = out.getJSONArray("node_ids")
            val mapped = JSONArray().apply {
                for (i in 0 until arr.length()) {
                    val hop = arr.getLong(i)
                    put(nodeMap[hop] ?: hop)
                }
            }
            out.put("node_ids", mapped).toString()
        } else {
            n.outbound
        }
        val meta = JSONObject(n.metadata)
        meta.optJSONObject("chain")?.let { c ->
            if (c.has("front")) c.put("front", mapTarget(c.getString("front")))
            if (c.has("landing")) c.put("landing", mapTarget(c.getString("landing")))
        }
        n.copy(id = nnId, groupId = ngId, outbound = nextOut, metadata = meta.toString())
    }
    val nextSettings = updated.settings.mapValues { (k, v) ->
        when (k) {
            "selectedNodeId" -> nodeMap[v.toLongOrNull()]?.toString() ?: v
            "selectedGroupId" -> groupMap[v.toLongOrNull()]?.toString() ?: v
            else -> if (k.startsWith("smart.") && k.endsWith(".target")) mapTarget(v) else v
        }
    }
    val nextRules = updated.rules.map { r -> r.copy(outbound = mapTarget(r.outbound)) }
    return updated.copy(groups = remappedGroups, nodes = remappedNodes, settings = nextSettings, rules = nextRules)
}

fun AppData.createDerivedChainNode(
    sourceId: Long,
    name: String,
    targets: ChainTargets,
    allocateId: () -> Long = derivedIdAllocator(),
): AppData {
    val req = JSONObject()
        .put("data", toRustJson())
        .put("sourceId", sourceId)
        .put("name", name)
        .put("targets", JSONObject().put("front", targets.front).put("landing", targets.landing))
    val result = AppData.fromRustJson(RustCore.callString("createDerivedChainNode", req))
    return remapNewIds(this, result, allocateId)
}

fun AppData.createDerivedChainGroup(
    sourceGroupId: Long,
    name: String,
    targets: ChainTargets,
    allocateId: () -> Long = derivedIdAllocator(),
): AppData {
    val req = JSONObject()
        .put("data", toRustJson())
        .put("sourceGroupId", sourceGroupId)
        .put("name", name)
        .put("targets", JSONObject().put("front", targets.front).put("landing", targets.landing))
    val result = AppData.fromRustJson(RustCore.callString("createDerivedChainGroup", req))
    val oldGroupIds = groups.map { it.id }.toSet()
    val normalized = result.copy(groups = result.groups.map { g -> if (g.id !in oldGroupIds) g.copy(options = JSONObject(g.options).toString()) else g })
    return remapNewIds(this, normalized, allocateId)
}

/** Generated members track source identities; source records and manually authored chains stay untouched. */
fun AppData.syncDerivedChains(allocateId: () -> Long = derivedIdAllocator()): AppData {
    if (groups.none { derivedGroupSourceId(it) > 0 }) return this
    val result = AppData.fromRustJson(RustCore.callString("syncDerivedChains", JSONObject().put("data", toRustJson())))
    return remapNewIds(this, result, allocateId)
}

fun validateDerivedChains(data: AppData) {
    RustCore.callBool("validateDerivedChains", JSONObject().put("data", data.toJson()))
}

fun derivedGroupUnavailableReason(data: AppData, group: Group): String? =
    RustCore.callNullableString(
        "derivedGroupUnavailableReason",
        JSONObject().put("data", data.toJson()).put("group", group.toRustJson()),
    )
