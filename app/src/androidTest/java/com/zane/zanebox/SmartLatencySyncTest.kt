package com.zane.zanebox

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.lifecycle.ViewModelProvider
import com.zane.zanebox.config.builtinSmartRuleFiles
import com.zane.zanebox.data.*
import com.zane.zanebox.ui.AppViewModel
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test

class SmartLatencySyncTest {
    @get:Rule val compose = createAndroidComposeRule<MainActivity>()
    private val vm get() = ViewModelProvider(compose.activity)[AppViewModel::class.java]

    private fun scrollTo(tag: String): SemanticsNodeInteraction {
        compose.onNodeWithTag("page_list").performScrollToNode(hasTestTag(tag))
        return compose.onNodeWithTag(tag)
    }

    @Test
    fun equivalentCrossGroupDuplicateSyncsLatencyWhileDistinctConfigStaysSeparateAndShowsGroupName() {
        val sameA = """{"type":"socks","server":"10.0.2.2","server_port":19081}"""
        val sameB = """{"server_port":19081,"server":"10.0.2.2","type":"socks","tag":"copy"}"""
        val distinct = """{"type":"socks","server":"10.0.2.2","server_port":19082}"""
        val data = AppData(
            nodes = listOf(
                Node(9101, 9101, "Goole 台湾", sameA, ping = 888, status = 3),
                Node(9102, 9102, "Goole 台湾", sameB, ping = 777, status = 3),
                Node(9103, 9103, "Goole 台湾", distinct, ping = 666, status = 3),
            ),
            groups = listOf(
                Group(9101, "主订阅", "https://example.test/a"),
                Group(9102, "自建节点"),
                Group(9103, "备用节点"),
            ),
            settings = builtinSmartRuleFiles.keys.associate { "smart.$it.target" to "off" } + mapOf(
                "appLanguage" to "zh-CN",
                "serviceMode" to "proxy",
                "selectedNodeId" to "9101",
                "selectedGroupId" to "9101",
                "browseGroupId" to "9101",
                "dnsRemote" to "local",
                "dnsDirect" to "local",
                "sniff" to "false",
                "testUrl" to "http://203.0.113.9:19080/test",
                "testTimeout" to "4000",
                "testConcurrency" to "12",
                "rulesUpdateInterval" to "off",
                "smartCustom.custom_proxy.name" to "代理",
                "smart.custom_proxy.target" to "node:9101",
                "smart.speed.target" to "node:9102",
                "smart.youtube.target" to "node:9103",
            ),
        )
        compose.runOnIdle { vm.edit { data } }
        compose.waitUntil(15000) { !vm.busy.value && vm.data.value.nodes.any { it.id == 9101L } }
        compose.onNodeWithTag("tab_1").performClick()

        scrollTo("smart_target_custom_proxy").assertTextEquals("主订阅 · Goole 台湾")
        scrollTo("smart_target_speed").assertTextEquals("自建节点 · Goole 台湾")
        scrollTo("smart_target_youtube").assertTextEquals("备用节点 · Goole 台湾")

        val previousBatch = vm.service.testProgress.value.batchId
        scrollTo("smart_latency_custom_proxy").performClick()
        compose.waitUntil(20000) {
            vm.service.testProgress.value.batchId > previousBatch &&
                !vm.service.testProgress.value.running &&
                vm.service.testResults.value.keys == setOf(9101L)
        }
        val measured = vm.service.testResults.value.getValue(9101L)
        assertTrue("measured ping must be positive: $measured", measured > 0)
        compose.waitUntil(10000) {
            val nodes = vm.data.value.nodes.associateBy { it.id }
            nodes[9101L]?.ping == measured && nodes[9102L]?.ping == measured
        }
        assertEquals(666, vm.data.value.nodes.first { it.id == 9103L }.ping)
        scrollTo("smart_latency_custom_proxy").assertTextEquals("$measured ms")
        scrollTo("smart_latency_speed").assertTextEquals("$measured ms")
        scrollTo("smart_latency_youtube").assertTextEquals("666 ms")
    }
}
