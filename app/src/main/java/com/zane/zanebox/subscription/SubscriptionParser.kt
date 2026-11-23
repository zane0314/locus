package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import org.json.JSONArray
import org.json.JSONObject

data class ParsedNode(
    val name: String,
    val outbound: String,
    val shareLink: String = "",
    val metadata: String = "{}",
)

data class ParseReport(
    val nodes: List<ParsedNode>,
    val skipped: Int = 0,
    val firstError: String? = null,
)

/** Full multi-format subscription parser delegated to Rust `locus_core`. */
object SubscriptionParser {
    fun parseReport(text: String): ParseReport {
        val obj = RustCore.callObject("parseSubscriptionReport", JSONObject().put("text", text))
        val arr = obj.getJSONArray("nodes")
        val nodes = (0 until arr.length()).map { i ->
            val n = arr.getJSONObject(i)
            ParsedNode(
                name = n.getString("name"),
                outbound = n.getString("outbound"),
                shareLink = n.optString("shareLink", ""),
                metadata = n.optString("metadata", "{}"),
            )
        }
        val firstErr = if (!obj.has("firstError") || obj.isNull("firstError")) null else obj.getString("firstError")
        return ParseReport(nodes = nodes, skipped = obj.optInt("skipped", 0), firstError = firstErr)
    }

    fun parse(text: String): List<ParsedNode> = parseReport(text).nodes

    fun applyHysteriaPorts(out: JSONObject, text: String) {
        require(text.length <= 65536) { "跳跃端口列表过长" }
        val ranges = text.split(',').map { raw ->
            val parts = raw.trim().replace('-', ':').split(':')
            require(parts.size in 1..2 && parts.all { it.matches(Regex("[0-9]{1,5}")) && it.toIntOrNull() in 1..65535 }) { "跳跃端口无效" }
            val start = parts[0].toInt()
            val end = parts.getOrElse(1) { parts[0] }.toInt()
            require(start <= end) { "跳跃端口范围倒序" }
            start to end
        }
        require(ranges.sumOf { it.second - it.first + 1 } <= 65535) { "跳跃端口总数超限" }
        if (ranges.size == 1 && ranges[0].first == ranges[0].second) {
            out.put("server_port", ranges[0].first)
            out.remove("server_ports")
        } else {
            out.remove("server_port")
            out.put("server_ports", JSONArray(ranges.map { "${it.first}:${it.second}" }))
        }
    }
}
