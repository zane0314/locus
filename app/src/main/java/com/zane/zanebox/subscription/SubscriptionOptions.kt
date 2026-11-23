package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import org.json.JSONArray
import org.json.JSONObject

data class SubscriptionOptions(
    val autoUpdate: Boolean = false,
    val autoUpdateDelay: Int = 1440,
    val updateWhenConnectedOnly: Boolean = false,
    val customUserAgent: String = "",
    val filterMode: Int = 0,
    val filterRegex: String = "",
    val deduplication: Boolean = false,
    val forceResolve: Boolean = false,
    val serverDnsResolver: String = "",
) {
    internal fun toJson(): JSONObject = JSONObject()
        .put("autoUpdate", autoUpdate)
        .put("autoUpdateDelay", autoUpdateDelay)
        .put("updateWhenConnectedOnly", updateWhenConnectedOnly)
        .put("customUserAgent", customUserAgent)
        .put("filterMode", filterMode)
        .put("filterRegex", filterRegex)
        .put("deduplication", deduplication)
        .put("forceResolve", forceResolve)
        .put("serverDnsResolver", serverDnsResolver)

    fun select(nodes: List<ParsedNode>): List<ParsedNode> {
        val req = JSONObject()
            .put("options", toJson())
            .put(
                "nodes",
                JSONArray().apply {
                    nodes.forEach { n ->
                        put(
                            JSONObject()
                                .put("name", n.name)
                                .put("outbound", n.outbound)
                                .put("shareLink", n.shareLink)
                                .put("metadata", n.metadata),
                        )
                    }
                },
            )
        val arr = RustCore.callArray("selectSubscriptionNodes", req)
        return (0 until arr.length()).map { i ->
            val n = arr.getJSONObject(i)
            ParsedNode(
                name = n.getString("name"),
                outbound = n.getString("outbound"),
                shareLink = n.optString("shareLink", ""),
                metadata = n.optString("metadata", "{}"),
            )
        }
    }

    companion object {
        fun parse(text: String): SubscriptionOptions {
            val o = RustCore.callObject("parseSubscriptionOptions", JSONObject().put("text", text))
            return SubscriptionOptions(
                autoUpdate = o.optBoolean("autoUpdate", false),
                autoUpdateDelay = o.optInt("autoUpdateDelay", 1440),
                updateWhenConnectedOnly = o.optBoolean("updateWhenConnectedOnly", false),
                customUserAgent = o.optString("customUserAgent", ""),
                filterMode = o.optInt("filterMode", 0),
                filterRegex = o.optString("filterRegex", ""),
                deduplication = o.optBoolean("deduplication", false),
                forceResolve = o.optBoolean("forceResolve", false),
                serverDnsResolver = o.optString("serverDnsResolver", ""),
            )
        }

        /** Include all user fields, including future ones; exclude our operational journal. */
        fun signature(text: String): String =
            RustCore.callString("subscriptionSignature", JSONObject().put("text", text))
    }
}
