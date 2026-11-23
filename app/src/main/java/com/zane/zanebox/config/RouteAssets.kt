package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.subscription.SubscriptionClient
import org.json.JSONArray
import org.json.JSONObject

/** Commit only while the source and cached rules still belong to this request. */
internal fun applySmartRuleUpdate(
    current: AppData,
    key: String,
    baseline: AppData,
    content: String,
    sourceUrl: String = baseline.setting("smartUrl.$key"),
    now: Long = System.currentTimeMillis(),
): AppData {
    val fields = listOf("smartUrl.$key", "smartRules.$key", "smartUpdated.$key", "smartCustom.$key.name")
    require(fields.all { current.settings[it] == baseline.settings[it] }) { "规则来源或内容已变更，已保留当前规则，请重新更新" }
    return current.copy(settings = current.settings + mapOf("smartUrl.$key" to sourceUrl, "smartRules.$key" to content, "smartUpdated.$key" to now.toString()))
}

internal suspend fun routeAssetUrl(kind: String, data: AppData, proxy: java.net.Proxy? = null): String {
    require(kind in listOf("geoip", "geosite"))
    val provider = data.setting("rulesProvider")
    if (provider == "4") return data.setting(if (kind == "geoip") "rulesGeoipUrl" else "rulesGeositeUrl").also { require(it.isNotBlank()) { "请配置资源 URL" } }
    val repo = when (provider) {
        "0" -> "SagerNet/sing-$kind"
        "1" -> "soffchen/sing-$kind"
        "2" -> { require(kind == "geoip") { "此更新源只提供 GeoIP" }; "Chocolate4U/Iran-sing-box" }
        "3" -> { require(kind == "geoip") { "此更新源只提供 GeoIP" }; "L11R/antizapret-sing-box-geo" }
        else -> error("未知资源更新源")
    }
    val release = JSONObject(SubscriptionClient.fetch("https://api.github.com/repos/$repo/releases/latest", settings = data.settings, proxy = proxy).body)
    val assets = release.getJSONArray("assets")
    return (0 until assets.length()).map { assets.getJSONObject(it) }.firstOrNull { it.optString("name") == "$kind.db" }?.getString("browser_download_url") ?: error("更新源没有 $kind.db")
}

/** Header/format verification delegated to Rust `locus_core`. */
internal fun geoAssetCode(kind: String, bytes: ByteArray): String =
    RustCore.geoAssetCode(kind, bytes)

internal val builtinSmartRuleFiles = mapOf(
    "speed" to listOf("Speed"),
    "youtube" to listOf("YouTube"),
    "telegram" to listOf("Telegram"),
    "netflix" to listOf("Netflix"),
    "disney" to listOf("Disney"),
    "tiktok" to listOf("TikTok"),
    "x" to listOf("Twitter"),
    "meta" to listOf("Instagram", "Facebook"),
    "spotify" to listOf("Spotify"),
    "google" to listOf("Google"),
    "ai" to listOf("OpenAI"),
)

internal fun normalizeSmartTarget(value: String) = if (value.isBlank() || value.startsWith("region:")) "proxy" else value
internal fun smartTarget(data: AppData, key: String): String =
    RustCore.callString("smartTarget", JSONObject().put("data", data.toJson()).put("key", key))

internal val unsupportedSmartRuleTypes = setOf("USER-AGENT", "IP-ASN", "OR")
internal fun builtinSmartRuleText(key: String, read: (String) -> String): String = builtinSmartRuleFiles[key].orEmpty().joinToString("\n") { read("anybox-rules/$it.list") }
internal fun supportedBuiltinSmartRules(raw: String): String = raw.lines().filter { it.substringBefore(',').trim().uppercase() !in unsupportedSmartRuleTypes }.joinToString("\n")
internal fun isBuiltinSmartRuleText(value: String, bundled: String): Boolean {
    fun normalized(text: String) = text.replace("\r\n", "\n").trim()
    return normalized(value) == normalized(bundled) || normalized(value) == normalized(supportedBuiltinSmartRules(bundled))
}

/** Missing defaults use bundled rules; an explicitly saved empty rule list stays empty. */
internal fun withBuiltinSmartRules(data: AppData, read: (String) -> String): AppData {
    val defaults = builtinSmartRuleFiles.keys.filter { key -> data.setting("smartUrl.$key").isBlank() }
        .mapNotNull { key ->
            val stored = data.settings["smartRules.$key"]
            if (stored != null && stored.isBlank()) return@mapNotNull null
            val bundled = builtinSmartRuleText(key, read)
            if (stored != null && !isBuiltinSmartRuleText(stored, bundled)) null
            else supportedBuiltinSmartRules(bundled).let { compatible -> if (stored == compatible) null else "smartRules.$key" to compatible }
        }.toMap()
    return if (defaults.isEmpty()) data else data.copy(settings = data.settings + defaults)
}

internal fun smartPolicyKeys(data: AppData): List<String> {
    val arr = RustCore.callArray("smartPolicyKeys", JSONObject().put("data", data.toJson()))
    return (0 until arr.length()).map { arr.getString(it) }
}

internal fun AppData.reorderSmartPolicies(requested: List<String>): AppData {
    val raw = RustCore.callString(
        "reorderSmartPolicies",
        JSONObject().put("data", toRustJson()).put("requested", JSONArray(requested)),
    )
    return AppData.fromRustJson(raw)
}
