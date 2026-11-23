package com.zane.zanebox.ui

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import org.json.JSONArray
import org.json.JSONObject

internal fun smartLowestNode(data: AppData, value: String, testing: Set<Long>): String? =
    RustCore.callNullableString(
        "smartLowestNode",
        JSONObject()
            .put("data", data.toJson())
            .put("value", value)
            .put("testing", JSONArray(testing.toList())),
    )

/** The bare value: a group's lowest successful latency, or a status word (测试中/未测试/失败); no prefix of any kind. */
internal fun smartLatencyLabel(data: AppData, value: String, testing: Set<Long>): String? =
    targetLatencyLabel(data, value, testing)
