package com.zane.zanebox

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.zane.zanebox.core.ExtraCoreClient
import com.zane.zanebox.core.ExtraCoreTestService
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.data.ZaneStore
import com.zane.zanebox.runtime.ServiceClient
import com.zane.zanebox.runtime.ZaneRuntime
import libcore.Libcore
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.FileInputStream

@RunWith(AndroidJUnit4::class)
class NotificationBadgeAndCoreVersionsTest {
    private val ins = InstrumentationRegistry.getInstrumentation()
    private val context get() = ins.targetContext

    private fun shell(command: String): String =
        ins.uiAutomation.executeShellCommand(command).use { p ->
            FileInputStream(p.fileDescriptor).bufferedReader().readText()
        }

    private fun waitFor(label: String, timeoutMs: Long = 30_000, predicate: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + timeoutMs
        while (!predicate()) {
            if (SystemClock.elapsedRealtime() > deadline) {
                throw AssertionError("Timeout waiting for: $label")
            }
            Thread.sleep(100)
        }
    }

    @Test
    fun legacyProxyChannelMigratesToProxyV2WithBadgeDisabled() {
        val manager = context.getSystemService(NotificationManager::class.java)
        // Simulate a pre-upgrade installation that still has legacy channel "proxy" with badge enabled.
        val legacy = NotificationChannel(
            ZaneRuntime.LEGACY_NOTIFICATION_CHANNEL_ID,
            "Locus 代理",
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            setShowBadge(true)
        }
        manager.createNotificationChannel(legacy)
        assertNotNull(manager.getNotificationChannel(ZaneRuntime.LEGACY_NOTIFICATION_CHANNEL_ID))
        assertTrue(manager.getNotificationChannel(ZaneRuntime.LEGACY_NOTIFICATION_CHANNEL_ID).canShowBadge())

        val store = ZaneStore(context)
        val client = ServiceClient(context)
        try {
            store.replace(
                AppData(
                    groups = listOf(Group(9501, "角标验证组")),
                    nodes = listOf(
                        Node(
                            9501,
                            9501,
                            "出口 A",
                            """{"type":"socks","server":"10.0.2.2","server_port":19081}"""
                        )
                    ),
                    settings = mapOf(
                        "appLanguage" to "zh-CN",
                        "serviceMode" to "proxy",
                        "proxyCore" to "sing-box",
                        "selectedNodeId" to "9501",
                        "selectedGroupId" to "9501",
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
            waitFor("proxy connected") {
                if (client.snapshot.value.state == 4) {
                    throw AssertionError("Proxy start failed: ${client.snapshot.value.error}")
                }
                client.snapshot.value.state == 2
            }

            waitFor("foreground notification active") {
                manager.activeNotifications.any { it.id == 1 }
            }

            // Verify NotificationManager API state.
            assertNull(
                "Legacy proxy channel must be deleted",
                manager.getNotificationChannel(ZaneRuntime.LEGACY_NOTIFICATION_CHANNEL_ID)
            )
            val channel = manager.getNotificationChannel(ZaneRuntime.NOTIFICATION_CHANNEL_ID)
            assertNotNull("proxy_v2 channel must exist", channel)
            assertFalse("proxy_v2 must disable badge", channel.canShowBadge())

            val active = manager.activeNotifications.first { it.id == 1 }.notification
            assertEquals(ZaneRuntime.NOTIFICATION_CHANNEL_ID, active.channelId)
            assertEquals(Notification.BADGE_ICON_NONE, active.badgeIconType)

            // Verify dumpsys notification agrees and save raw evidence excerpt for com.zane.locus.
            val dump = shell("dumpsys notification --noredact")
            val locusExcerpt = dump.lineSequence()
                .filter { line ->
                    line.contains("com.zane.locus") ||
                        line.contains("proxy_v2") ||
                        line.contains("mId='proxy'") ||
                        line.contains("badgeIcon") ||
                        line.contains("canShowBadge") ||
                        line.contains("mShowBadge")
                }
                .take(120)
                .joinToString("\n")
            context.getExternalFilesDir(null)?.let { dir ->
                File(dir, "dumpsys_notification_badge.txt").writeText(locusExcerpt)
            }

            val proxyV2Line = dump.lineSequence().firstOrNull { it.contains("mId='proxy_v2'") }
            assertNotNull("dumpsys notification must list proxy_v2 channel", proxyV2Line)
            assertTrue(
                "dumpsys notification proxy_v2 must have mShowBadge=false: $proxyV2Line",
                proxyV2Line!!.contains("mShowBadge=false") && !proxyV2Line.contains("mDeleted=true")
            )
            val activeLegacyLines = dump.lineSequence()
                .filter { it.contains("mId='proxy',") && !it.contains("mDeleted=true") }
                .toList()
            assertTrue(
                "dumpsys notification must not have an active legacy proxy channel: $activeLegacyLines",
                activeLegacyLines.isEmpty()
            )
            val notifLine = dump.lineSequence().firstOrNull {
                it.contains("Notification(channel=proxy_v2")
            }
            assertNotNull("dumpsys notification must list active Notification(channel=proxy_v2)", notifLine)
            // On Android Notification.toString(), badgeIcon is printed as badgeIcon=NONE/SMALL/LARGE or omitted when 0;
            // we assert it is not SMALL/LARGE and cross-check active.badgeIconType == BADGE_ICON_NONE (0) above.
            assertFalse(
                "dumpsys notification must not show a non-NONE badgeIcon: $notifLine",
                notifLine!!.contains("badgeIcon=SMALL") ||
                    notifLine.contains("badgeIcon=LARGE") ||
                    notifLine.contains("badgeIcon=1") ||
                    notifLine.contains("badgeIcon=2")
            )
        } finally {
            runCatching { client.stop() }
            waitFor("proxy stopped", 15_000) { client.snapshot.value.state == 0 }
            client.close()
            store.close()
        }
    }

    @Test
    fun allThreeNativeCoresReportTargetVersions() {
        val singBoxVersion = Libcore.versionBox()
        assertTrue(
            "Expected sing-box 1.14.4, got: $singBoxVersion",
            singBoxVersion.contains("sing-box: 1.14.4")
        )
        // Always query libextra_cores.so via its isolated :extraTest process so the main
        // process never loads two Go runtimes simultaneously.
        ExtraCoreClient(context, ExtraCoreTestService::class.java).use { extraClient ->
            assertEquals("26.11.9", extraClient.version("xray"))
            assertEquals("v1.19.34", extraClient.version("mihomo"))
        }
    }
}
