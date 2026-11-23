package com.zane.zanebox

import android.app.Application
import android.net.Uri
import android.os.SystemClock
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.zane.zanebox.backup.BackupManager
import com.zane.zanebox.data.*
import com.zane.zanebox.runtime.ServiceClient
import com.zane.zanebox.ui.AppViewModel
import com.zane.zanebox.ui.PendingImport
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.collect
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import java.util.concurrent.CopyOnWriteArrayList

@RunWith(AndroidJUnit4::class)
class V101AcceptanceInstrumentedTest {
    @get:Rule val compose = createEmptyComposeRule()
    private val ins = InstrumentationRegistry.getInstrumentation()
    private val context get() = ins.targetContext

    private fun shell(command: String): String =
        ins.uiAutomation.executeShellCommand(command).use { p ->
            java.io.FileInputStream(p.fileDescriptor).bufferedReader().readText()
        }

    private fun waitFor(label: String, timeout: Long = 30000, predicate: () -> Boolean) {
        val end = SystemClock.elapsedRealtime() + timeout
        while (!predicate()) {
            if (SystemClock.elapsedRealtime() > end) fail("超时 $label")
            Thread.sleep(100)
        }
    }

    private fun proxyHttpGet(url: String, port: Int = 2080): String {
        val conn = URL(url).openConnection(Proxy(Proxy.Type.HTTP, InetSocketAddress("127.0.0.1", port))) as HttpURLConnection
        return try {
            conn.connectTimeout = 5000
            conn.readTimeout = 5000
            conn.inputStream.bufferedReader().use { it.readText() }
        } finally {
            conn.disconnect()
        }
    }

    @Test
    fun backupZipShowsPreviewDialogCancelPreservesAndConfirmRestores() {
        val store = ZaneStore(context)
        val initial = AppData(
            groups = listOf(Group(9001, "现有组")),
            nodes = listOf(Node(9001, 9001, "现有节点", """{"type":"socks","server":"10.0.2.2","server_port":19081}""")),
            settings = mapOf(
                "appLanguage" to "zh-CN",
                "factoryRouteDefaultsVersion" to "1",
                "selectedNodeId" to "9001",
                "selectedGroupId" to "9001"
            )
        )
        store.replace(initial)
        val backupData = AppData(
            groups = listOf(Group(9101, "备份组 A"), Group(9102, "备份组 B")),
            nodes = listOf(
                Node(9101, 9101, "备份节点 1", """{"type":"socks","server":"10.0.2.2","server_port":19081}"""),
                Node(9102, 9102, "备份节点 2", """{"type":"socks","server":"10.0.2.2","server_port":19082}""")
            ),
            rules = listOf(RouteRule(9101, "备份规则", domains = "backup.example.test", outbound = "direct")),
            settings = mapOf("appLanguage" to "zh-CN", "selectedNodeId" to "9101", "selectedGroupId" to "9101")
        )
        val zipFile = File(context.cacheDir, "v101-backup-acceptance.zip")
        zipFile.writeBytes(BackupManager(context).export(backupData))
        val uri = Uri.fromFile(zipFile)

        var scenario: ActivityScenario<MainActivity>? = null
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            lateinit var vm: AppViewModel
            scenario.onActivity { vm = ViewModelProvider(it)[AppViewModel::class.java] }
            waitFor("VM 初始化") { !vm.busy.value && vm.data.value.groups.singleOrNull()?.id == 9001L }

            ins.runOnMainSync { vm.readText(uri) }
            waitFor("备份预览弹窗") { vm.pendingImport.value is PendingImport.Backup }
            compose.onNodeWithText("用外部备份替换全部数据？").assertIsDisplayed()
            compose.onNodeWithText("备份含 2 个组、2 个节点、1 条规则。", substring = true).assertIsDisplayed()

            compose.onNodeWithTag("import_cancel").performClick()
            waitFor("取消备份弹窗") { vm.pendingImport.value == null }
            assertEquals(listOf(9001L), store.snapshot().groups.map { it.id })
            assertEquals(0, store.snapshot().rules.size)

            ins.runOnMainSync { vm.readText(uri) }
            waitFor("再次打开备份预览弹窗") { vm.pendingImport.value is PendingImport.Backup }
            compose.onNodeWithTag("import_confirm").performClick()
            waitFor("备份确认恢复完成", 30000) {
                !vm.busy.value && vm.pendingImport.value == null &&
                    store.snapshot().groups.size == 2 && store.snapshot().nodes.size == 2 && store.snapshot().rules.size == 1
            }
            val restored = store.snapshot()
            assertEquals(setOf("备份组 A", "备份组 B"), restored.groups.map { it.name }.toSet())
            assertEquals("备份规则", restored.rules.single().name)
        } finally {
            zipFile.delete()
            scenario?.close()
            store.close()
        }
    }

    @Test
    fun updateAllGroupsReportsExactSummaryCounts() {
        val store = ZaneStore(context)
        store.replace(
            AppData(
                groups = listOf(
                    Group(9201, "可达订阅", "http://10.0.2.2:19080/subscription"),
                    Group(9202, "失败订阅", "http://127.0.0.1:1/unreachable-sub")
                ),
                settings = mapOf(
                    "appLanguage" to "zh-CN",
                    "serviceMode" to "proxy",
                    "dnsRemote" to "local",
                    "testUrl" to "http://10.0.2.2:19080/test",
                    "rulesUpdateInterval" to "off"
                )
            )
        )
        val app = context.applicationContext as Application
        val holder = ViewModelStore()
        lateinit var vm: AppViewModel
        val messages = CopyOnWriteArrayList<String>()
        val observer = CoroutineScope(SupervisorJob() + Dispatchers.Default)
        try {
            ins.runOnMainSync {
                vm = AppViewModel(app)
                holder.put("v101UpdateAll", vm)
            }
            observer.launch { vm.message.collect { if (it.isNotBlank()) messages.add(it) } }
            waitFor("VM 初始化") { !vm.busy.value && vm.data.value.groups.size == 2 }
            ins.runOnMainSync { vm.updateAllGroups() }
            waitFor("全部订阅更新汇总", 45000) {
                !vm.busy.value && messages.any { it.startsWith("全部更新结束：") }
            }
            val summary = messages.last { it.startsWith("全部更新结束：") }
            assertEquals("全部更新结束：1 组测速完成，0 组测速未完成，1 组更新失败", summary)
            assertEquals(2, store.snapshot().nodes.size)
        } finally {
            observer.cancel()
            ins.runOnMainSync { holder.clear() }
            store.close()
        }
    }

    @Test
    fun homeAutoSelectSubtitleUpdatesWithIntervalAndAutoTestPowerModeSwitchesSafely() {
        val store = ZaneStore(context)
        store.replace(
            AppData(
                groups = listOf(Group(9301, "自动间隔组", "https://example.test/sub")),
                nodes = listOf(
                    Node(9301, 9301, "节点 A", """{"type":"socks","server":"10.0.2.2","server_port":19081}"""),
                    Node(9302, 9301, "节点 B", """{"type":"socks","server":"10.0.2.2","server_port":19082}""")
                ),
                settings = mapOf(
                    "appLanguage" to "zh-CN",
                    "factoryRouteDefaultsVersion" to "1",
                    "homeAutoSelect" to "true",
                    "urlTestInterval" to "5m",
                    "autoTestPowerMode" to "normal",
                    "serviceMode" to "proxy",
                    "proxyCore" to "sing-box",
                    "dnsRemote" to "local",
                    "sniff" to "false",
                    "smart.ai.target" to "auto",
                    "smartRules.ai" to "IP-CIDR,203.0.113.9/32",
                    "rulesUpdateInterval" to "off"
                )
            )
        )
        var scenario: ActivityScenario<MainActivity>? = null
        lateinit var vm: AppViewModel
        try {
            scenario = ActivityScenario.launch(MainActivity::class.java)
            scenario.onActivity { vm = ViewModelProvider(it)[AppViewModel::class.java] }
            waitFor("VM 初始化") { !vm.busy.value && vm.data.value.nodes.size == 2 }

            compose.waitForIdle()
            compose.waitUntil(10000) {
                compose.onAllNodesWithTag("home_auto").fetchSemanticsNodes().isNotEmpty()
            }
            try {
                compose.onNodeWithTag("home_auto").performScrollTo().assertIsDisplayed()
                    .assertTextContains("每 5 分钟 测速", substring = true)
            } catch (t: Throwable) {
                throw AssertionError("home_auto 5m check failed; tree:\n" + compose.onRoot(useUnmergedTree = true).printToString(), t)
            }
            ins.runOnMainSync { vm.setting("urlTestInterval", "10m") }
            waitFor("间隔切为 10m") { vm.data.value.setting("urlTestInterval") == "10m" }
            compose.waitUntil(10000) {
                compose.onAllNodesWithText("每 10 分钟 测速", substring = true).fetchSemanticsNodes().isNotEmpty()
            }
            compose.onNodeWithTag("home_auto").performScrollTo().assertIsDisplayed()
                .assertTextContains("每 10 分钟 测速", substring = true)

            fun hasPendingAutoTestAlarm(): Boolean =
                shell("dumpsys alarm")
                    .substringBefore("Removal history:")
                    .substringBefore("Top Alarms:")
                    .contains("${context.packageName}.AUTO_TEST")

            ins.runOnMainSync { vm.service.start() }
            waitFor("代理连接成功", 30000) {
                if (vm.service.snapshot.value.state == 4) fail(vm.service.snapshot.value.error)
                vm.service.snapshot.value.state == 2
            }
            waitFor("normal 模式注册 AUTO_TEST 闹钟", 15000) {
                hasPendingAutoTestAlarm()
            }

            ins.runOnMainSync { vm.setting("autoTestPowerMode", "saving") }
            waitFor("saving 模式保存并取消 AUTO_TEST 闹钟", 30000) {
                vm.data.value.setting("autoTestPowerMode") == "saving" &&
                    vm.service.snapshot.value.state == 2 &&
                    !hasPendingAutoTestAlarm()
            }

            ins.runOnMainSync { vm.setting("autoTestPowerMode", "normal") }
            waitFor("normal 模式保存并恢复 AUTO_TEST 闹钟", 30000) {
                vm.data.value.setting("autoTestPowerMode") == "normal" &&
                    vm.service.snapshot.value.state == 2 &&
                    hasPendingAutoTestAlarm()
            }
        } finally {
            runCatching {
                vm.service.stop()
                waitFor("停止代理", 15000) { vm.service.snapshot.value.state == 0 }
            }
            scenario?.close()
            store.close()
        }
    }

    @Test
    fun extraCoresForwardTrafficAndKillingExtraMainStopsConnectionSafely() {
        val store = ZaneStore(context)
        val client = ServiceClient(context)
        val events = CopyOnWriteArrayList<String>()
        val observer = CoroutineScope(SupervisorJob() + Dispatchers.IO)
        observer.launch { client.events.collect { events.add(it) } }
        try {
            // 1. Verify Xray (:extraMain) real traffic forwarding
            store.replace(
                AppData(
                    groups = listOf(Group(9401, "三内核组")),
                    nodes = listOf(
                        Node(9401, 9401, "出口 A", """{"type":"socks","server":"10.0.2.2","server_port":19081}"""),
                        Node(9402, 9401, "出口 B", """{"type":"socks","server":"10.0.2.2","server_port":19082}""")
                    ),
                    settings = mapOf(
                        "appLanguage" to "zh-CN",
                        "serviceMode" to "proxy",
                        "proxyCore" to "xray",
                        "selectedNodeId" to "9401",
                        "selectedGroupId" to "9401",
                        "dnsRemote" to "local",
                        "dnsDirect" to "local",
                        "sniff" to "false",
                        "statsEnabled" to "false",
                        "rulesUpdateInterval" to "off"
                    )
                )
            )
            client.connect()
            client.start()
            waitFor("Xray 本地代理启动", 30000) {
                if (client.snapshot.value.state == 4) fail("Xray 启动失败：${client.snapshot.value.error}")
                client.snapshot.value.state == 2
            }
            val xrayPid = shell("pidof ${context.packageName}:extraMain").trim()
            assertTrue("Xray 未启动 :extraMain 进程：$xrayPid", xrayPid.matches(Regex("[0-9]+")))
            assertEquals("EXIT_A", proxyHttpGet("http://203.0.113.9:19080/probe"))
            client.stop()
            waitFor("Xray 停止", 15000) { client.snapshot.value.state == 0 }

            // 2. Verify mihomo (:extraMain) real traffic forwarding + unexpected process kill protection
            events.clear()
            store.update {
                it.copy(
                    settings = it.settings + mapOf(
                        "proxyCore" to "mihomo",
                        "selectedNodeId" to "9402"
                    )
                )
            }
            client.start()
            waitFor("mihomo 本地代理启动", 30000) {
                if (client.snapshot.value.state == 4) fail("mihomo 启动失败：${client.snapshot.value.error}")
                client.snapshot.value.state == 2
            }
            val mihomoPid = shell("pidof ${context.packageName}:extraMain").trim()
            assertTrue("mihomo 未启动 :extraMain 进程：$mihomoPid", mihomoPid.matches(Regex("[0-9]+")))
            assertEquals("EXIT_B", proxyHttpGet("http://203.0.113.9:19080/probe"))

            // Force-kill :extraMain and verify main service stops and reports the disconnect message
            shell("run-as ${context.packageName} kill -9 $mihomoPid")
            waitFor("附加内核被杀后主服务自动断开", 20000) {
                client.snapshot.value.state == 0 && events.any { it.contains("附加内核服务中断，连接已关闭") }
            }

            // 3. Verify sing-box real traffic forwarding without :extraMain
            store.update {
                it.copy(
                    settings = it.settings + mapOf(
                        "proxyCore" to "sing-box",
                        "selectedNodeId" to "9401"
                    )
                )
            }
            client.start()
            waitFor("sing-box 本地代理启动", 30000) {
                if (client.snapshot.value.state == 4) fail("sing-box 启动失败：${client.snapshot.value.error}")
                client.snapshot.value.state == 2
            }
            assertEquals("", shell("pidof ${context.packageName}:extraMain").trim())
            assertEquals("EXIT_A", proxyHttpGet("http://203.0.113.9:19080/probe"))
            client.stop()
            waitFor("sing-box 停止", 15000) { client.snapshot.value.state == 0 }
        } finally {
            runCatching { client.stop() }
            Thread.sleep(500)
            client.close()
            observer.cancel()
            store.close()
        }
    }
}
