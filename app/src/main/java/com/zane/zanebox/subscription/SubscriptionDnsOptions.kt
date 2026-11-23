package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject

/** Applies the original serverDnsResolver option to dialing node hostnames, delegated to Rust `locus_core`. */
object SubscriptionDnsOptions {
    fun server(address: String, tag: String): JSONObject =
        RustCore.callObject("subscriptionDnsServer", JSONObject().put("address", address).put("tag", tag))

    internal fun bootstrapped(server: JSONObject, address: String): JSONObject =
        RustCore.callObject("subscriptionDnsBootstrapped", JSONObject().put("server", server).put("address", address))

    fun apply(root: JSONObject, data: AppData, sourceNodeIds: Map<String, Long> = emptyMap()) {
        val resolvers = data.groups.associate { it.id to JSONObject(it.options).optString("serverDnsResolver").trim() }
            .filterValues { it.isNotBlank() }
        if (resolvers.isEmpty()) return
        val definitions = mutableMapOf<Long, JSONObject>()
        val dns = root.optJSONObject("dns") ?: JSONObject().also { root.put("dns", it) }
        val servers = dns.optJSONArray("servers") ?: JSONArray().also { dns.put("servers", it) }
        val used = mutableSetOf<Long>()
        listOfNotNull(root.optJSONArray("outbounds"), root.optJSONArray("endpoints")).forEach { array ->
            for (i in 0 until array.length()) {
                val outbound = array.getJSONObject(i)
                val tag = outbound.optString("tag")
                val id = sourceNodeIds[tag] ?: Regex("node-([0-9]+)(?:-.*)?").matchEntire(tag)?.groupValues?.get(1)?.toLongOrNull() ?: continue
                val groupId = data.nodes.find { it.id == id }?.groupId ?: continue
                if (groupId !in resolvers) continue
                val host = outbound.optString("server")
                val hasDomain = host.isNotBlank() && !host.contains(':') && host.split('.').any { it.toIntOrNull() == null }
                val peerDomain = outbound.optJSONArray("peers")?.let { peers ->
                    (0 until peers.length()).any {
                        val address = peers.getJSONObject(it).optString("address")
                        address.isNotBlank() && !address.contains(':') && address.split('.').any { part -> part.toIntOrNull() == null }
                    }
                } ?: false
                if (hasDomain || peerDomain) {
                    val definition = definitions.getOrPut(groupId) { server(resolvers.getValue(groupId), "subscription-dns-$groupId") }
                    outbound.put("domain_resolver", definition.getString("tag"))
                    used.add(groupId)
                }
            }
        }
        used.forEach { servers.put(definitions.getValue(it)) }
    }
}
