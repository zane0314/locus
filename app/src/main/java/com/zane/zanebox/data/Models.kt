package com.zane.zanebox.data

import org.json.JSONArray
import org.json.JSONObject

data class Node(val id:Long, val groupId:Long, val name:String, val outbound:String, val shareLink:String="", val ping:Int=-1, val tx:Long=0, val rx:Long=0, val order:Int=0, val status:Int=0, val metadata:String="{}")
data class Group(val id:Long, val name:String, val subscriptionUrl:String="", val enabled:Boolean=true, val order:Int=0, val updatedAt:Long=0, val userInfo:String="", val frontProxy:Long=0, val landingProxy:Long=0, val options:String="{}")
data class RouteRule(val id:Long, val name:String, val domains:String="", val packages:String="", val ipCidrs:String="", val outbound:String="proxy", val enabled:Boolean=true, val order:Int=0, val advanced:String="{}", val prioritize:Boolean=false)
data class MergeGroup(val id:Long, val name:String, val nodeIds:List<Long> = emptyList(), val groupIds:List<Long> = emptyList(), val mode:String="selector", val selectedId:Long=0)
data class AppData(val nodes:List<Node> = emptyList(), val groups:List<Group> = emptyList(), val rules:List<RouteRule> = emptyList(), val merges:List<MergeGroup> = emptyList(), val settings:Map<String,String> = emptyMap()) {
    val selectedNodeId:Long get() = setting("selectedNodeId","0").toLongOrNull() ?: 0L
    val selectedGroupId:Long get() = setting("selectedGroupId","0").toLongOrNull() ?: 0L
    fun setting(key:String, default:String=SettingDefaults.value(key)):String = settings[key] ?: default
    fun bool(key:String, default:Boolean=SettingDefaults.value(key,"false").toBoolean()):Boolean = settings[key]?.toBooleanStrictOrNull() ?: default
    fun resolvedIpv6Mode():String {
        val valid=setOf("ipv4_only","prefer_ipv4","prefer_ipv6","ipv6_only")
        return settings["dnsStrategy"]?.takeIf { it in valid }
            ?: settings["ipv6Mode"]?.takeIf { it in valid }
            ?: if(bool("ipv6",false))"prefer_ipv4" else "ipv4_only"
    }
    fun ipv6Enabled():Boolean = resolvedIpv6Mode()!="ipv4_only"
    fun toJson():String = JSONObject().apply {
        put("format",1)
        put("nodes",JSONArray().apply { nodes.forEach { n -> put(JSONObject().apply { put("id",n.id);put("groupId",n.groupId);put("name",n.name);put("outbound",JSONObject(n.outbound));put("shareLink",n.shareLink);put("ping",n.ping);put("tx",n.tx);put("rx",n.rx);put("order",n.order);put("status",n.status);put("metadata",JSONObject(n.metadata)) }) } })
        put("groups",JSONArray().apply { groups.forEach { g -> put(JSONObject().apply { put("id",g.id);put("name",g.name);put("subscriptionUrl",g.subscriptionUrl);put("enabled",g.enabled);put("order",g.order);put("updatedAt",g.updatedAt);put("userInfo",g.userInfo);put("frontProxy",g.frontProxy);put("landingProxy",g.landingProxy);put("options",JSONObject(g.options)) }) } })
        put("rules",JSONArray().apply { rules.forEach { r -> put(JSONObject().apply { put("id",r.id);put("name",r.name);put("domains",r.domains);put("packages",r.packages);put("ipCidrs",r.ipCidrs);put("outbound",r.outbound);put("enabled",r.enabled);put("order",r.order);put("advanced",JSONObject(r.advanced));put("prioritize",r.prioritize) }) } })
        put("merges",JSONArray().apply { merges.forEach { m -> put(JSONObject().apply { put("id",m.id);put("name",m.name);put("nodeIds",JSONArray(m.nodeIds));put("groupIds",JSONArray(m.groupIds));put("mode",m.mode);put("selectedId",m.selectedId) }) } })
        put("settings",JSONObject(settings))
    }.toString()
    /** Wire format for Rust JNI that preserves raw JSON strings (`outbound`/`metadata`/`options`/`advanced`) byte-for-byte. */
    internal fun toRustJson():String = JSONObject().apply {
        put("format",1)
        put("nodes",JSONArray().apply { nodes.forEach { n -> put(JSONObject().apply { put("id",n.id);put("groupId",n.groupId);put("name",n.name);put("outbound",n.outbound);put("shareLink",n.shareLink);put("ping",n.ping);put("tx",n.tx);put("rx",n.rx);put("order",n.order);put("status",n.status);put("metadata",n.metadata) }) } })
        put("groups",JSONArray().apply { groups.forEach { g -> put(JSONObject().apply { put("id",g.id);put("name",g.name);put("subscriptionUrl",g.subscriptionUrl);put("enabled",g.enabled);put("order",g.order);put("updatedAt",g.updatedAt);put("userInfo",g.userInfo);put("frontProxy",g.frontProxy);put("landingProxy",g.landingProxy);put("options",g.options) }) } })
        put("rules",JSONArray().apply { rules.forEach { r -> put(JSONObject().apply { put("id",r.id);put("name",r.name);put("domains",r.domains);put("packages",r.packages);put("ipCidrs",r.ipCidrs);put("outbound",r.outbound);put("enabled",r.enabled);put("order",r.order);put("advanced",r.advanced);put("prioritize",r.prioritize) }) } })
        put("merges",JSONArray().apply { merges.forEach { m -> put(JSONObject().apply { put("id",m.id);put("name",m.name);put("nodeIds",JSONArray(m.nodeIds));put("groupIds",JSONArray(m.groupIds));put("mode",m.mode);put("selectedId",m.selectedId) }) } })
        put("settings",JSONObject(settings))
    }.toString()
    internal fun validate(previous:AppData?=null):AppData {
        val req = JSONObject().put("data", toRustJson())
        if (previous != null) req.put("previous", previous.toRustJson())
        com.zane.zanebox.core.RustCore.callBool("validateAppData", req)
        return this
    }
    companion object {
        internal fun fromRustJson(text:String, validateResult:Boolean=false):AppData {
            val root=JSONObject(text)
            fun <T> records(key:String,read:(JSONObject)->T):List<T> { val a=root.optJSONArray(key) ?: JSONArray();return (0 until a.length()).map { read(a.getJSONObject(it)) } }
            fun longs(a:JSONArray?):List<Long> = if(a==null) emptyList() else (0 until a.length()).map { a.getLong(it) }
            fun rawField(o:JSONObject,key:String,fallback:String):String = when(val v=o.opt(key)) { is String->v; is JSONObject->v.toString(); else->fallback }
            val s=root.optJSONObject("settings") ?: JSONObject()
            val data=AppData(
                records("nodes") { Node(it.getLong("id"),it.getLong("groupId"),it.getString("name"),rawField(it,"outbound","{}"),it.optString("shareLink"),it.optInt("ping",-1),it.optLong("tx"),it.optLong("rx"),it.optInt("order"),it.optInt("status"),rawField(it,"metadata","{}")) },
                records("groups") { Group(it.getLong("id"),it.getString("name"),it.optString("subscriptionUrl"),it.optBoolean("enabled",true),it.optInt("order"),it.optLong("updatedAt"),it.optString("userInfo"),it.optLong("frontProxy"),it.optLong("landingProxy"),rawField(it,"options","{}")) },
                records("rules") { RouteRule(it.getLong("id"),it.getString("name"),it.optString("domains"),it.optString("packages"),it.optString("ipCidrs"),it.optString("outbound","proxy"),it.optBoolean("enabled",true),it.optInt("order"),rawField(it,"advanced","{}"),it.optBoolean("prioritize")) },
                records("merges") { MergeGroup(it.getLong("id"),it.getString("name"),longs(it.optJSONArray("nodeIds")),longs(it.optJSONArray("groupIds")),it.optString("mode","selector"),it.optLong("selectedId")) },
                s.keys().asSequence().associateWith { s.getString(it) }
            )
            return if(validateResult) data.validate() else data
        }
        fun fromJson(text:String):AppData {
            require(text.toByteArray(Charsets.UTF_8).size<=64*1024*1024) { "数据超过64MiB" }
            val root=JSONObject(text);require(root.optInt("format",1)==1) { "不支持的数据版本" }
            fun <T> records(key:String,read:(JSONObject)->T):List<T> { val a=root.optJSONArray(key) ?: JSONArray();require(a.length()<=100000);return (0 until a.length()).map { read(a.getJSONObject(it)) } }
            fun longs(a:JSONArray?):List<Long> = if(a==null) emptyList() else (0 until a.length()).map { a.getLong(it) }
            val s=root.optJSONObject("settings") ?: JSONObject()
            return AppData(
                records("nodes") { Node(it.getLong("id"),it.getLong("groupId"),it.getString("name"),it.getJSONObject("outbound").toString(),it.optString("shareLink"),it.optInt("ping",-1),it.optLong("tx"),it.optLong("rx"),it.optInt("order"),it.optInt("status"),it.optJSONObject("metadata")?.toString() ?: "{}") },
                records("groups") { Group(it.getLong("id"),it.getString("name"),it.optString("subscriptionUrl"),it.optBoolean("enabled",true),it.optInt("order"),it.optLong("updatedAt"),it.optString("userInfo"),it.optLong("frontProxy"),it.optLong("landingProxy"),it.optJSONObject("options")?.toString() ?: "{}") },
                records("rules") { RouteRule(it.getLong("id"),it.getString("name"),it.optString("domains"),it.optString("packages"),it.optString("ipCidrs"),it.optString("outbound","proxy"),it.optBoolean("enabled",true),it.optInt("order"),it.optJSONObject("advanced")?.toString() ?: "{}",it.optBoolean("prioritize")) },
                records("merges") { MergeGroup(it.getLong("id"),it.getString("name"),longs(it.optJSONArray("nodeIds")),longs(it.optJSONArray("groupIds")),it.optString("mode","selector"),it.optLong("selectedId")) },
                s.keys().asSequence().associateWith { s.getString(it) }
            ).validate()
        }
    }
}

/** AnyBox 2.1.9 Theme.getTheme(1..22) colorPrimary values, in the original preference order. */
internal val themePresetColors=listOf("#f44336","#fb7299","#e91e63","#9c27b0","#673ab7","#3f51b5","#0066cc","#03a9f4","#00bcd4","#009688","#4caf50","#8bc34a","#cddc39","#ffeb3b","#ffc107","#ff9800","#ff5722","#795548","#9e9e9e","#607d8b","#2b2b2b","#00b96b")
