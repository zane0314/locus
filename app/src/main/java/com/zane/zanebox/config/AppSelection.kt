package com.zane.zanebox.config

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

internal data class InstalledApp(val name: String, val packageName: String, val system: Boolean = false, val launchable: Boolean = true)

internal fun appPackages(text: String): Set<String> = text.split(Regex("[\\s,;]+")).filter { it.isNotBlank() }.toSet()

internal fun filterVpnPackages(data: AppData, packages: Iterable<String>): Set<String> {
    val arr = RustCore.callArray(
        "filterVpnPackages",
        JSONObject().put("data", data.toJson()).put("packages", JSONArray(packages.toList())),
    )
    return (0 until arr.length()).map { arr.getString(it) }.toSet()
}

internal fun effectiveSmartPackages(data: AppData, key: String, installed: Set<String>? = null): Set<String> {
    val req = JSONObject().put("data", data.toJson()).put("key", key)
    if (installed != null) req.put("installed", JSONArray(installed.toList()))
    val arr = RustCore.callArray("effectiveSmartPackages", req)
    return (0 until arr.length()).map { arr.getString(it) }.toSet()
}

internal fun commonAppPackages(apps: List<InstalledApp>, self: String, bypass: Boolean = false): Set<String> = apps.filter {
    it.packageName != self && (CommonProxyApps.matches(it.packageName, it.name) || it.packageName in CommonProxyApps.companions) != bypass
}.map { it.packageName }.toSet()

internal fun visibleApps(apps: List<InstalledApp>, selected: Set<String>, query: String, showSystem: Boolean, scope: AppData? = null): List<InstalledApp> {
    val allowed = scope?.let { filterVpnPackages(it, apps.map { app -> app.packageName }) }
    return apps.filter { (showSystem || it.launchable && !it.system) && (query.isBlank() || it.name.contains(query, true) || it.packageName.contains(query, true)) && (allowed == null || it.packageName in allowed) }
        .sortedWith(compareByDescending<InstalledApp> { it.packageName in selected }.thenBy { it.name.lowercase(Locale.ROOT) }.thenBy { it.packageName })
}

internal data class AppPaste(val accepted: Set<String>, val skipped: Int)

internal fun parseAppPaste(text: String, installed: Set<String>, scope: AppData? = null): AppPaste {
    val req = JSONObject()
        .put("text", text)
        .put("installed", JSONArray(installed.toList()))
    if (scope != null) req.put("scope", scope.toJson())
    val obj = RustCore.callObject("parseAppPaste", req)
    val acceptedArr = obj.getJSONArray("accepted")
    val accepted = (0 until acceptedArr.length()).map { acceptedArr.getString(it) }.toSet()
    return AppPaste(accepted, obj.getInt("skipped"))
}
