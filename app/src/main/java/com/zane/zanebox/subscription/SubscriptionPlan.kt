package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.Group
import org.json.JSONObject

internal fun Group.toRustJson(): JSONObject = JSONObject()
    .put("id", id)
    .put("name", name)
    .put("subscriptionUrl", subscriptionUrl)
    .put("enabled", enabled)
    .put("order", order)
    .put("updatedAt", updatedAt)
    .put("userInfo", userInfo)
    .put("frontProxy", frontProxy)
    .put("landingProxy", landingProxy)
    .put("options", options)

object SubscriptionPlan {
    fun dueAt(group: Group): Long =
        RustCore.callLong("subscriptionDueAt", JSONObject().put("group", group.toRustJson()))

    fun nextAt(group: Group): Long =
        RustCore.callLong("subscriptionNextAt", JSONObject().put("group", group.toRustJson()))

    fun shouldUpdate(group: Group, now: Long, connected: Boolean): Boolean =
        RustCore.callBool(
            "subscriptionShouldUpdate",
            JSONObject()
                .put("group", group.toRustJson())
                .put("now", now)
                .put("connected", connected),
        )

    fun scheduled(group: Group): Boolean =
        RustCore.callBool("subscriptionScheduled", JSONObject().put("group", group.toRustJson()))
}
