package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.*
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.json.JSONArray
import org.json.JSONObject

/** One transformation pipeline for UI and durable background updates; state transitions execute in Rust `locus_core`. */
object SubscriptionUpdater {
    suspend fun prepare(
        nodes: List<ParsedNode>,
        options: SubscriptionOptions,
        ipv6: Boolean,
        familyStrategy: String = "",
        lookup: suspend (String) -> List<String>,
    ): List<ParsedNode> = coroutineScope {
        val strategy = if (!ipv6) "ipv4_only" else familyStrategy
        require(strategy in listOf("", "prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only")) { "节点解析地址族策略无效" }
        val cache = java.util.concurrent.ConcurrentHashMap<String, List<String>>()
        options.select(nodes).chunked(4).flatMap { batch ->
            batch.map { parsed ->
                async {
                    currentCoroutineContext().ensureActive()
                    val out = JSONObject(parsed.outbound)
                    val meta = JSONObject(parsed.metadata)
                    meta.put("subscriptionIdentity", NodeIdentity.key(parsed.outbound))
                    val targets = mutableListOf<Pair<JSONObject, String>>()
                    if (out.has("server")) targets.add(out to "server")
                    out.optJSONArray("peers")?.let { peers ->
                        for (i in 0 until peers.length()) targets.add(peers.getJSONObject(i) to "address")
                    }
                    if (options.forceResolve) targets.forEach { (target, key) ->
                        val host = target.optString(key)
                        if (host.isNotBlank() && !literal(host)) try {
                            val addresses = cache[host] ?: lookup(host).also { cache[host] = it }
                            currentCoroutineContext().ensureActive()
                            val ipv4 = addresses.firstOrNull { !it.contains(':') }
                            val ipv6Address = addresses.firstOrNull { it.contains(':') }
                            val address = when (strategy) {
                                "ipv4_only" -> ipv4
                                "ipv6_only" -> ipv6Address
                                "prefer_ipv6" -> ipv6Address ?: ipv4
                                else -> ipv4 ?: ipv6Address
                            }
                            if (address != null) {
                                if (target === out) {
                                    out.optJSONObject("tls")
                                        ?.takeIf { it.optBoolean("enabled") && it.optString("server_name").isBlank() }
                                        ?.put("server_name", host)
                                    meta.put("subscriptionOriginalServer", host)
                                }
                                target.put(key, address)
                            } else {
                                require(options.serverDnsResolver.isBlank() || options.serverDnsResolver == "local") {
                                    "指定节点 DNS 没有允许地址族的解析结果，已保留原节点"
                                }
                                meta.put("subscriptionResolveError", "没有允许地址族的解析结果，保留域名")
                            }
                        } catch (e: CancellationException) {
                            throw e
                        } catch (_: Exception) {
                            require(options.serverDnsResolver.isBlank() || options.serverDnsResolver == "local") {
                                "指定节点 DNS 解析失败，已保留原节点"
                            }
                            meta.put("subscriptionResolveError", "节点域名解析失败，保留域名")
                        }
                    }
                    parsed.copy(outbound = out.toString(), metadata = meta.toString())
                }
            }.awaitAll()
        }
    }

    private fun literal(host: String): Boolean =
        host.contains(':') || host.split('.').let { parts -> parts.size == 4 && parts.all { it.toIntOrNull() in 0..255 } }

    fun begin(current: AppData, expected: Group, request: String, now: Long, automatic: Boolean = false): AppData {
        val req = JSONObject()
            .put("current", current.toRustJson())
            .put("expected", expected.toRustJson())
            .put("request", request)
            .put("now", now)
            .put("automatic", automatic)
        return AppData.fromRustJson(RustCore.callString("subscriptionBegin", req))
    }

    fun apply(
        current: AppData,
        baseline: Group,
        parsed: List<ParsedNode>,
        userInfo: String,
        now: Long,
        nextId: () -> Long,
    ): AppData {
        // Probe the first allocated ID from caller's allocator when custom IDs are expected, else let Rust allocate sequentially.
        val parsedArr = JSONArray().apply {
            parsed.forEach { p ->
                put(
                    JSONObject()
                        .put("name", p.name)
                        .put("outbound", p.outbound)
                        .put("shareLink", p.shareLink)
                        .put("metadata", p.metadata),
                )
            }
        }
        val req = JSONObject()
            .put("current", current.toRustJson())
            .put("baseline", baseline.toRustJson())
            .put("parsed", parsedArr)
            .put("userInfo", userInfo)
            .put("now", now)
        val raw = RustCore.callString("subscriptionApply", req)
        val result = AppData.fromRustJson(raw)
        // Re-map any newly created IDs through caller's nextId() allocator so SQLite sequence / custom test allocators stay monotonic.
        val existingIds = current.nodes.map { it.id }.toSet()
        val newlyCreated = result.nodes.filter { it.id !in existingIds }
        if (newlyCreated.isEmpty()) return result
        val remap = newlyCreated.associate { it.id to nextId() }
        if (remap.all { (k, v) -> k == v }) return result
        val remappedNodes = result.nodes.map { n ->
            val newId = remap[n.id] ?: n.id
            val outObj = JSONObject(n.outbound)
            val newOutbound = if (outObj.optString("type") == "chain" && outObj.has("node_ids")) {
                val arr = outObj.getJSONArray("node_ids")
                val updated = JSONArray().apply {
                    for (i in 0 until arr.length()) {
                        val hop = arr.getLong(i)
                        put(remap[hop] ?: hop)
                    }
                }
                outObj.put("node_ids", updated).toString()
            } else {
                n.outbound
            }
            val metaObj = JSONObject(n.metadata)
            val newMeta = if (metaObj.has("chainSourceNodeId")) {
                val src = metaObj.optLong("chainSourceNodeId")
                remap[src]?.let { metaObj.put("chainSourceNodeId", it).toString() } ?: n.metadata
            } else {
                n.metadata
            }
            n.copy(id = newId, outbound = newOutbound, metadata = newMeta)
        }
        val selNode = result.selectedNodeId
        val nextSettings = remap[selNode]?.let { result.settings + ("selectedNodeId" to it.toString()) } ?: result.settings
        return result.copy(nodes = remappedNodes, settings = nextSettings)
    }

    fun record(
        current: AppData,
        groupId: Long,
        state: String,
        now: Long,
        error: String = "",
        nextAttempt: Long = 0,
        request: String? = null,
    ): AppData {
        val req = JSONObject()
            .put("current", current.toRustJson())
            .put("groupId", groupId)
            .put("state", state)
            .put("now", now)
            .put("error", error)
            .put("nextAttempt", nextAttempt)
        if (request != null) req.put("request", request)
        return AppData.fromRustJson(RustCore.callString("subscriptionRecord", req))
    }
}

/** A chain losing a hop cannot silently become a shorter, differently routed chain. */
internal fun nodeRemovalIds(nodes: List<Node>, ids: Set<Long>): Set<Long> {
    val arr = JSONArray().apply {
        nodes.forEach { n ->
            put(
                JSONObject()
                    .put("id", n.id)
                    .put("groupId", n.groupId)
                    .put("name", n.name)
                    .put("outbound", n.outbound)
                    .put("shareLink", n.shareLink)
                    .put("ping", n.ping)
                    .put("tx", n.tx)
                    .put("rx", n.rx)
                    .put("order", n.order)
                    .put("status", n.status)
                    .put("metadata", n.metadata),
            )
        }
    }
    val res = RustCore.callArray("nodeRemovalIds", JSONObject().put("nodes", arr).put("ids", JSONArray(ids.toList())))
    return (0 until res.length()).map { res.getLong(it) }.toSet()
}
