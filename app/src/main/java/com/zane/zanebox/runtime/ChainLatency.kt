package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** Each selector owns its candidates; landing tags already contain the selected front and body detour. */
internal data class ChainProbe(val tag: String, val candidates: List<String>, val unavailable: String)

internal data class ChainProbePlan(val nodeId: Long, val front: ChainProbe?, val landing: ChainProbe?) {
    companion object {
        private fun parseProbe(obj: JSONObject?): ChainProbe? {
            if (obj == null) return null
            val arr = obj.getJSONArray("candidates")
            return ChainProbe(
                tag = obj.getString("tag"),
                candidates = (0 until arr.length()).map { arr.getString(it) },
                unavailable = obj.getString("unavailable"),
            )
        }

        fun read(config: String): List<ChainProbePlan> {
            val arr = RustCore.callArray("readChainProbePlans", JSONObject().put("config", config))
            return (0 until arr.length()).map { i ->
                val item = arr.getJSONObject(i)
                ChainProbePlan(
                    nodeId = item.getLong("nodeId"),
                    front = parseProbe(item.optJSONObject("front")),
                    landing = parseProbe(item.optJSONObject("landing")),
                )
            }
        }
    }
}

/** A round never reuses landing delays from before the front selection, including failed rounds. */
internal inline fun chooseChainProbe(
    probe: ChainProbe,
    test: (String) -> Int,
    select: (String, String) -> Boolean,
    active: () -> Boolean,
): String {
    var winner = probe.unavailable
    var best = Int.MAX_VALUE
    for (candidate in probe.candidates) {
        check(active()) { "代理链测速已过期" }
        val delay = test(candidate)
        check(active()) { "代理链测速已过期" }
        if (delay > 0 && delay < best) {
            best = delay
            winner = candidate
        }
    }
    check(active()) { "代理链测速已过期" }
    check(select(probe.tag, winner)) { "代理链选择无法应用：${probe.tag}" }
    return winner
}

/** Tags differ per subject even when a front candidate's raw route is identical. */
internal fun chainProbeKeys(config: String): Map<String, String> {
    val obj = RustCore.callObject("chainProbeKeys", JSONObject().put("config", config))
    return obj.keys().asSequence().associateWith { obj.getString(it) }
}

internal data class ChainParentProbe(val tag: String, val members: List<String>, val dependencies: Set<Long>)

internal fun chainParentProbes(config: String): List<ChainParentProbe> {
    val arr = RustCore.callArray("chainParentProbes", JSONObject().put("config", config))
    return (0 until arr.length()).map { i ->
        val item = arr.getJSONObject(i)
        val memArr = item.getJSONArray("members")
        val depArr = item.getJSONArray("dependencies")
        ChainParentProbe(
            tag = item.getString("tag"),
            members = (0 until memArr.length()).map { memArr.getString(it) },
            dependencies = (0 until depArr.length()).map { depArr.getLong(it) }.toSet(),
        )
    }
}

internal fun routedChainNodes(config: String): Set<Long> {
    val arr = RustCore.callArray("routedChainNodes", JSONObject().put("config", config))
    return (0 until arr.length()).map { arr.getLong(it) }.toSet()
}
