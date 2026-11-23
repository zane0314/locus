package com.zane.zanebox.subscription

import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.Node
import org.json.JSONObject

/** Legacy SOCKS5 encoded-username repair delegated to Rust `locus_core`. */
fun repairLegacySocksCredentials(node: Node): Node {
    val req = JSONObject().put(
        "node",
        JSONObject()
            .put("id", node.id)
            .put("groupId", node.groupId)
            .put("name", node.name)
            .put("outbound", node.outbound)
            .put("shareLink", node.shareLink)
            .put("ping", node.ping)
            .put("tx", node.tx)
            .put("rx", node.rx)
            .put("order", node.order)
            .put("status", node.status)
            .put("metadata", node.metadata),
    )
    val res = RustCore.callObject("repairLegacySocksCredentials", req)
    val updated = res.getString("outbound")
    return if (updated == node.outbound) node else node.copy(outbound = updated)
}
