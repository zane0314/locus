package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject

enum class Purpose { MAIN, TEST, EXPORT }

/** Complete sing-box / multi-core configuration compiler delegated to Rust `locus_core`. */
object ConfigBuilder {
    class PreparedTestBatch internal constructor(private val snapshotJson: String) {
        fun build(nodeId: Long): String = RustCore.callString(
            "buildConfig",
            JSONObject()
                .put("data", snapshotJson)
                .put("purpose", Purpose.TEST.name)
                .put("testNodeId", nodeId)
                .put("runtimeSecret", ""),
        )
    }

    fun prepareTestBatch(data: AppData): PreparedTestBatch = PreparedTestBatch(data.toJson())

    internal fun normalizeTransportHost(outbound: JSONObject) {
        val result = RustCore.callObject("normalizeTransportHost", JSONObject().put("outbound", outbound))
        replaceJsonObject(outbound, result)
    }

    fun ruleSetFormat(url: String): String =
        RustCore.callString("ruleSetFormat", JSONObject().put("url", url))

    fun ruleCompatibilityWarnings(service: String, rules: String): List<String> =
        ruleCompatibilityWarnings(service, rules) { key, type -> "$key：此规则格式暂不支持 $type；该类条目保留但不参与匹配，其余有效规则仍生效" }

    fun ruleCompatibilityWarnings(service: String, rules: String, format: (String, String) -> String): List<String> {
        val arr = RustCore.callArray("ruleCompatibilityWarningTypes", JSONObject().put("rules", rules))
        return (0 until arr.length()).map { format(service, arr.getString(it)) }
    }

    fun warnings(data: AppData): List<String> {
        val arr = RustCore.callArray("configWarnings", JSONObject().put("data", data.toJson()))
        return (0 until arr.length()).map { arr.getString(it) }
    }

    fun targetWarnings(data: AppData): List<String> {
        val arr = RustCore.callArray("configTargetWarnings", JSONObject().put("data", data.toJson()))
        return (0 until arr.length()).map { arr.getString(it) }
    }

    fun smartTargetNodeIds(data: AppData, target: String): List<Long> {
        val arr = RustCore.callArray(
            "smartTargetNodeIds",
            JSONObject().put("data", data.toJson()).put("target", target),
        )
        return (0 until arr.length()).map { arr.getLong(it) }
    }

    fun build(
        data: AppData,
        purpose: Purpose = Purpose.MAIN,
        testNodeId: Long = 0,
        runtimeSecret: String = "",
        installedPackages: Set<String>? = null,
    ): String {
        val req = JSONObject()
            .put("data", data.toJson())
            .put("purpose", purpose.name)
            .put("testNodeId", testNodeId)
            .put("runtimeSecret", runtimeSecret)
        if (installedPackages != null) req.put("installedPackages", JSONArray(installedPackages.toList()))
        return RustCore.callString("buildConfig", req)
    }

    internal fun normalizeAdvanced(rule: JSONObject) {
        val result = RustCore.callObject("normalizeAdvanced", JSONObject().put("rule", rule))
        replaceJsonObject(rule, result)
    }

    internal fun mergeJson(base: JSONObject, custom: JSONObject) {
        val result = RustCore.callObject("mergeJson", JSONObject().put("base", base).put("custom", custom))
        replaceJsonObject(base, result)
    }

    fun validate(config: String) {
        RustCore.callBool("validateConfig", JSONObject().put("config", config))
    }

    private fun replaceJsonObject(target: JSONObject, replacement: JSONObject) {
        val existingKeys = target.keys().asSequence().toList()
        existingKeys.forEach { target.remove(it) }
        replacement.keys().forEach { key -> target.put(key, replacement.get(key)) }
    }
}
