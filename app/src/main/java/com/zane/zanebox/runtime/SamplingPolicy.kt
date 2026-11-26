package com.zane.zanebox.runtime

import com.zane.zanebox.core.RustCore
import org.json.JSONObject

/** The native tracker counts the route's chosen tag, including service/group targets, once. */
internal object SamplingPolicy {
    fun pauseCore(deviceIdle: Boolean, subscriptionBusy: Boolean, wireGuard: Boolean) =
        deviceIdle && !subscriptionBusy && !wireGuard

    fun interval(interactive: Boolean, observed: Boolean) = when {
        interactive && observed -> 1000L
        interactive || observed -> 10000L
        else -> 60000L
    }

    fun controllerPollInterval(interactive: Boolean, observed: Boolean) = when {
        interactive && observed -> 5000L
        interactive || observed -> 30000L
        else -> 120000L
    }

    fun persistInterval(interactive: Boolean, observed: Boolean) =
        if (!interactive && !observed) 180000L else 60000L

    /** Coalesces staggered URLTest groups due within one window into a single wakeup. */
    fun coalescedAutoTestDelay(dueMs: Collection<Long>, intervalMs: Long): Long? {
        val valid = dueMs.filter { it >= 0L }
        val minDue = valid.minOrNull() ?: return null
        val window = minOf(15_000L, (intervalMs / 4L).coerceAtLeast(0L))
        return valid.filter { it <= minDue + window }.maxOrNull() ?: minDue
    }

    fun trackedTags(root: JSONObject): List<String> {
        val arr = RustCore.callArray("samplingTrackedTags", JSONObject().put("config", root.toString()))
        return (0 until arr.length()).map { arr.getString(it) }
    }

    fun activeUrlTestGroups(config: String): Set<String> {
        val arr = RustCore.callArray("activeUrlTestGroups", JSONObject().put("config", config))
        return (0 until arr.length()).mapTo(LinkedHashSet()) { arr.getString(it) }
    }
}
