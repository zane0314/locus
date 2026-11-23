package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.subscription.toRustJson
import org.json.JSONObject

data class ChainTargets(val front: String = "none", val landing: String = "none")

internal fun Node.toRustJson(): JSONObject = JSONObject()
    .put("id", id)
    .put("groupId", groupId)
    .put("name", name)
    .put("outbound", outbound)
    .put("shareLink", shareLink)
    .put("ping", ping)
    .put("tx", tx)
    .put("rx", rx)
    .put("order", order)
    .put("status", status)
    .put("metadata", metadata)

private fun JSONObject.toChainTargets() = ChainTargets(getString("front"), getString("landing"))

fun groupChainTargets(group: Group): ChainTargets =
    RustCore.callObject("groupChainTargets", JSONObject().put("group", group.toRustJson())).toChainTargets()

fun nodeChainTargets(node: Node): ChainTargets =
    RustCore.callObject("nodeChainTargets", JSONObject().put("node", node.toRustJson())).toChainTargets()

fun effectiveChainTargets(data: AppData, node: Node): ChainTargets =
    RustCore.callObject(
        "effectiveChainTargets",
        JSONObject().put("data", data.toJson()).put("node", node.toRustJson()),
    ).toChainTargets()

fun Group.withChainTargets(targets: ChainTargets): Group = copy(
    options = JSONObject(options)
        .put("chain", JSONObject().put("front", targets.front).put("landing", targets.landing))
        .toString(),
    frontProxy = targets.front.takeIf { it.startsWith("node:") }?.substringAfter(':')?.toLongOrNull() ?: 0,
    landingProxy = targets.landing.takeIf { it.startsWith("node:") }?.substringAfter(':')?.toLongOrNull() ?: 0,
)

fun Node.withChainTargets(targets: ChainTargets): Node = copy(
    metadata = JSONObject(metadata)
        .put("chain", JSONObject().put("front", targets.front).put("landing", targets.landing))
        .toString(),
)

fun validateChainTargetSyntax(data: AppData) {
    RustCore.callBool("validateChainTargetSyntax", JSONObject().put("data", data.toJson()))
}

fun validateChainSettings(data: AppData) {
    RustCore.callBool("validateChainSettings", JSONObject().put("data", data.toJson()))
}

fun chainUnavailableReason(data: AppData, node: Node): String? =
    RustCore.callNullableString(
        "chainUnavailableReason",
        JSONObject().put("data", data.toJson()).put("node", node.toRustJson()),
    )

fun chainPathLabels(data: AppData, nodeId: Long): List<String> {
    val arr = RustCore.callArray(
        "chainPathLabels",
        JSONObject().put("data", data.toJson()).put("nodeId", nodeId),
    )
    return (0 until arr.length()).map { arr.getString(it) }
}
