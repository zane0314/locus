package com.zane.zanebox

import com.zane.zanebox.backup.BackupManager
import com.zane.zanebox.config.ConfigBuilder
import com.zane.zanebox.config.Purpose
import com.zane.zanebox.core.MultiCorePlanner
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.subscription.SubscriptionParser
import com.zane.zanebox.ui.updateNodeField
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.Base64

class Locus107RegressionTest {
    @Test fun hysteria2GeckoBareUriAndClashYamlAndMihomoPlanner() {
        val raw = """
            hysteria2://secret-auth@198.51.100.10:443/?sni=edge.example.test&alpn=h3&insecure=0&obfs=gecko&obfs-password=gecko-pass#Gecko-Basic
            hy2://secret-auth@[2001:db8::10]:443?sni=edge6.example.test&obfs=gecko&obfs-password=gecko-pass-2&obfs-min-packet-size=600&obfs-max-packet-size=1100#Gecko-Sized
            hysteria2://secret-auth@198.51.100.11:443?sni=edge.example.test&obfs=gecko#Gecko-NoPass
        """.trimIndent()
        val report = SubscriptionParser.parseReport(raw)
        assertEquals(2, report.nodes.size)
        assertEquals(1, report.skipped)
        val nodes = report.nodes

        val basic = JSONObject(nodes[0].outbound)
        assertEquals("hysteria2", basic.getString("type"))
        val basicObfs = basic.getJSONObject("obfs")
        assertEquals("gecko", basicObfs.getString("type"))
        assertEquals("gecko-pass", basicObfs.getString("password"))
        assertFalse(basicObfs.has("min_packet_size"))
        assertFalse(basicObfs.has("max_packet_size"))

        val sized = JSONObject(nodes[1].outbound)
        assertEquals("2001:db8::10", sized.getString("server"))
        val sizedObfs = sized.getJSONObject("obfs")
        assertEquals("gecko", sizedObfs.getString("type"))
        assertEquals("gecko-pass-2", sizedObfs.getString("password"))
        assertEquals(600, sizedObfs.getInt("min_packet_size"))
        assertEquals(1100, sizedObfs.getInt("max_packet_size"))

        assertTrue(
            "gecko without obfs-password must be rejected fail-closed",
            runCatching {
                SubscriptionParser.parse("hysteria2://secret-auth@198.51.100.11:443?sni=edge.example.test&obfs=gecko#Gecko-NoPass")
            }.isFailure,
        )
        assertTrue(
            "gecko with only min_packet_size > default max (1200) must be rejected",
            runCatching {
                SubscriptionParser.parse("hysteria2://secret-auth@198.51.100.11:443?sni=edge.example.test&obfs=gecko&obfs-password=p&obfs-min-packet-size=1300#BadMin")
            }.isFailure,
        )
        assertTrue(
            "gecko with only max_packet_size < default min (512) must be rejected",
            runCatching {
                SubscriptionParser.parse("hysteria2://secret-auth@198.51.100.11:443?sni=edge.example.test&obfs=gecko&obfs-password=p&obfs-max-packet-size=400#BadMax")
            }.isFailure,
        )
        assertTrue(
            "gecko packet size > 2048 must be rejected",
            runCatching {
                SubscriptionParser.parse("hysteria2://secret-auth@198.51.100.11:443?sni=edge.example.test&obfs=gecko&obfs-password=p&obfs-max-packet-size=2500#OverWire")
            }.isFailure,
        )
        assertTrue(
            "Clash YAML gecko without obfs-password must be rejected fail-closed",
            runCatching {
                SubscriptionParser.parse("proxies:\n  - {name: BadClash, type: hysteria2, server: 198.51.100.12, port: 443, password: pw, obfs: gecko}")
            }.isFailure,
        )

        val clashYaml = """
            proxies:
              - name: Clash-Gecko-Basic
                type: hysteria2
                server: 198.51.100.12
                port: 443
                password: pw1
                sni: clash1.example.test
                obfs: gecko
                obfs-password: gp1
              - name: Clash-Gecko-Sized
                type: hysteria2
                server: 198.51.100.13
                port: 8443
                password: pw2
                sni: clash2.example.test
                obfs: gecko
                obfs-password: gp2
                obfs-min-packet-size: 512
                obfs-max-packet-size: 1180
        """.trimIndent()
        val clashNodes = SubscriptionParser.parse(clashYaml)
        assertEquals(2, clashNodes.size)
        val clashBasicObfs = JSONObject(clashNodes[0].outbound).getJSONObject("obfs")
        assertEquals("gecko", clashBasicObfs.getString("type"))
        assertEquals("gp1", clashBasicObfs.getString("password"))
        assertFalse(clashBasicObfs.has("min_packet_size"))
        val clashSizedObfs = JSONObject(clashNodes[1].outbound).getJSONObject("obfs")
        assertEquals("gecko", clashSizedObfs.getString("type"))
        assertEquals("gp2", clashSizedObfs.getString("password"))
        assertEquals(512, clashSizedObfs.getInt("min_packet_size"))
        assertEquals(1180, clashSizedObfs.getInt("max_packet_size"))

        val group = Group(1, "GeckoGroup")
        val appData = AppData(
            nodes = listOf(
                Node(10, 1, nodes[0].name, nodes[0].outbound),
                Node(11, 1, nodes[1].name, nodes[1].outbound),
            ),
            groups = listOf(group),
            settings = mapOf("selectedNodeId" to "10", "selectedGroupId" to "1", "proxyCore" to "mihomo"),
        )
        val basicPlan = MultiCorePlanner.plan(ConfigBuilder.build(appData, Purpose.TEST, testNodeId = 10))!!
        val basicProxy = JSONObject(basicPlan.extraCoreConfig).getJSONObject("coreConfig").getJSONArray("proxies").getJSONObject(0)
        assertEquals("gecko", basicProxy.getString("obfs"))
        assertEquals("gecko-pass", basicProxy.getString("obfs-password"))
        assertFalse(basicProxy.has("obfs-min-packet-size"))
        assertFalse(basicProxy.has("obfs-max-packet-size"))

        val sizedPlan = MultiCorePlanner.plan(ConfigBuilder.build(appData, Purpose.TEST, testNodeId = 11))!!
        val sizedProxy = JSONObject(sizedPlan.extraCoreConfig).getJSONObject("coreConfig").getJSONArray("proxies").getJSONObject(0)
        assertEquals("gecko", sizedProxy.getString("obfs"))
        assertEquals("gecko-pass-2", sizedProxy.getString("obfs-password"))
        assertEquals(600, sizedProxy.getInt("obfs-min-packet-size"))
        assertEquals(1100, sizedProxy.getInt("obfs-max-packet-size"))
    }

    @Test fun nodeEditorUpdatesHysteria2GeckoObfsCleanly() {
        var outbound = """{"type":"hysteria2","server":"edge.example.test","server_port":443,"password":"auth"}"""
        outbound = updateNodeField(outbound, "obfs.type", "gecko", "text")
        outbound = updateNodeField(outbound, "obfs.password", "g-secret", "password")
        outbound = updateNodeField(outbound, "obfs.min_packet_size", "520", "number")
        outbound = updateNodeField(outbound, "obfs.max_packet_size", "1150", "number")
        val geckoObj = JSONObject(outbound).getJSONObject("obfs")
        assertEquals("gecko", geckoObj.getString("type"))
        assertEquals("g-secret", geckoObj.getString("password"))
        assertEquals(520, geckoObj.getInt("min_packet_size"))
        assertEquals(1150, geckoObj.getInt("max_packet_size"))

        try {
            updateNodeField(outbound, "obfs.min_packet_size", "0", "number")
            fail("Zero min_packet_size must be rejected")
        } catch (_: IllegalArgumentException) {
        }

        // Switching to salamander must drop gecko-only packet size fields while keeping password.
        outbound = updateNodeField(outbound, "obfs.type", "salamander", "text")
        val salamanderObj = JSONObject(outbound).getJSONObject("obfs")
        assertEquals("salamander", salamanderObj.getString("type"))
        assertEquals("g-secret", salamanderObj.getString("password"))
        assertFalse(salamanderObj.has("min_packet_size"))
        assertFalse(salamanderObj.has("max_packet_size"))

        // Clearing obfs.type must remove the entire obfs object.
        outbound = updateNodeField(outbound, "obfs.type", "", "text")
        assertFalse(JSONObject(outbound).has("obfs"))
    }

    @Test fun ipv6ModeAndDnsStrategyAndLegacyBackupStayConsistent() {
        val node = Node(1, 1, "Socks", """{"type":"socks","server":"198.51.100.1","server_port":1080}""")
        val group = Group(1, "Default")

        val onlyIpv6Mode = AppData(
            nodes = listOf(node),
            groups = listOf(group),
            settings = mapOf(
                "selectedNodeId" to "1",
                "selectedGroupId" to "1",
                "serviceMode" to "vpn",
                "ipv6Mode" to "prefer_ipv6",
            ),
        )
        val cfg1 = JSONObject(ConfigBuilder.build(onlyIpv6Mode, Purpose.MAIN))
        assertEquals("prefer_ipv6", onlyIpv6Mode.resolvedIpv6Mode())
        assertTrue(onlyIpv6Mode.ipv6Enabled())
        val tun1 = cfg1.getJSONArray("inbounds").getJSONObject(0)
        val addr1 = tun1.getJSONArray("address")
        assertEquals("TUN must include IPv6 CIDR when ipv6Mode=prefer_ipv6", 2, addr1.length())
        assertEquals("prefer_ipv6", cfg1.getJSONObject("dns").getString("strategy"))

        val onlyDnsStrategy = AppData(
            nodes = listOf(node),
            groups = listOf(group),
            settings = mapOf(
                "selectedNodeId" to "1",
                "selectedGroupId" to "1",
                "serviceMode" to "vpn",
                "ipv6" to "true",
                "dnsStrategy" to "ipv4_only",
            ),
        )
        assertEquals("ipv4_only", onlyDnsStrategy.resolvedIpv6Mode())
        assertFalse(onlyDnsStrategy.ipv6Enabled())
        val cfg2 = JSONObject(ConfigBuilder.build(onlyDnsStrategy, Purpose.MAIN))
        val tun2 = cfg2.getJSONArray("inbounds").getJSONObject(0)
        assertEquals("TUN must stay IPv4-only when dnsStrategy=ipv4_only", 1, tun2.getJSONArray("address").length())
        assertEquals("ipv4_only", cfg2.getJSONObject("dns").getString("strategy"))

        // Legacy backup numeric ipv6Mode=2 must normalize both dnsStrategy and ipv6Mode to prefer_ipv6.
        fun preference(key: String, type: Int, bytes: ByteArray): String {
            val out = ByteArrayOutputStream()
            fun leInt(v: Int) { out.write(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(v).array()) }
            fun align() { while (out.size() % 4 != 0) out.write(0) }
            leInt(key.length); out.write(key.toByteArray(Charsets.UTF_16LE)); out.write(byteArrayOf(0, 0)); align()
            leInt(type); leInt(bytes.size); out.write(bytes); align()
            return Base64.getEncoder().encodeToString(out.toByteArray())
        }
        val legacySettings = JSONArray().put(preference("ipv6Mode", 3, ByteBuffer.allocate(4).putInt(2).array()))
        val legacyRoot = JSONObject()
            .put("version", 1)
            .put("profiles", JSONArray())
            .put("groups", JSONArray())
            .put("rules", JSONArray())
            .put("settings", legacySettings)
        val restored = BackupManager().`import`(legacyRoot.toString().toByteArray())
        assertEquals("prefer_ipv6", restored.setting("dnsStrategy"))
        assertEquals("prefer_ipv6", restored.setting("ipv6Mode"))
        assertTrue(restored.bool("ipv6"))
    }
}
