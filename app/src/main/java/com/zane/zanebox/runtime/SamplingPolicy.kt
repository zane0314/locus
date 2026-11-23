package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** The native tracker counts the route's chosen tag, including service/group targets, once. */
internal object SamplingPolicy {
    fun pauseCore(deviceIdle: Boolean, subscriptionBusy: Boolean, wireGuard: Boolean) =
        deviceIdle && !subscriptionBusy && !wireGuard

    fun interval(interactive: Boolean, observed: Boolean) =
        if (interactive && observed) 1000L else 10000L

    fun trackedTags(root: JSONObject): List<String> {
        val arr = RustCore.callArray("samplingTrackedTags", JSONObject().put("config", root.toString()))
        return (0 until arr.length()).map { arr.getString(it) }
    }

    fun activeUrlTestGroups(config: String): Set<String> {
        val arr = RustCore.callArray("activeUrlTestGroups", JSONObject().put("config", config))
        return (0 until arr.length()).mapTo(LinkedHashSet()) { arr.getString(it) }
    }
}
