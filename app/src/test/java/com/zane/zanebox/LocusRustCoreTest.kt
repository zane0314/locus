package com.zane.zanebox

import com.zane.zanebox.config.ConfigBuilder
import com.zane.zanebox.config.Purpose
import com.zane.zanebox.core.MultiCorePlanner
import com.zane.zanebox.core.RustCore
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.data.SettingDefaults
import com.zane.zanebox.subscription.SubscriptionParser
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class LocusRustCoreTest {
    @Test fun rustCoreLoadsAndReportsVersion() {
        val version = RustCore.version()
        assertTrue("Expected locus_core version, got $version", version.startsWith("locus_core "))
    }

    @Test fun defaultBrandSettingsUseLinearAndSingBox() {
        assertEquals("linear", SettingDefaults.value("uiSkin"))
        assertEquals("sing-box", SettingDefaults.value("proxyCore"))
    }

    @Test fun incomingLinkSupportsLocusAndLegacyZaneboxSchemes() {
        val locusSub = IncomingLink.parse("locus://subscription?url=https%3A%2F%2Fexample.test%2Fsub&name=LocusSub")
        assertEquals("https://example.test/sub", locusSub.subscriptionUrl)
        assertEquals("LocusSub", locusSub.name)

        val locusImport = IncomingLink.parse("locus://import?text=socks5%3A%2F%2Fexample.test%3A1080")
        assertEquals("socks5://example.test:1080", locusImport.text)

        val legacySub = IncomingLink.parse("zanebox://subscription?url=https%3A%2F%2Fexample.test%2Fold&name=Old")
        assertEquals("https://example.test/old", legacySub.subscriptionUrl)
    }

    @Test fun multiCorePlannerPlansXrayVlessRealityAndMihomoHysteria2() {
        val parsed = SubscriptionParser.parse(
            """
            vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?security=reality&sni=www.microsoft.com&pbk=PUBKEY&sid=abcd&fp=chrome&type=xhttp&host=cdn.example.com&path=%2Fxh#VLESS-XHTTP
            hysteria2://pw@hy2.example.com:443?sni=hy2.example.com&insecure=1&obfs=salamander&obfs-password=ob#HY2-Node
            """.trimIndent(),
        )
        assertEquals(2, parsed.size)
        val group = Group(1, "Default", "https://example.test/sub")
        val vlessNode = Node(10, 1, parsed[0].name, parsed[0].outbound, parsed[0].shareLink, metadata = parsed[0].metadata)
        val hy2Node = Node(11, 1, parsed[1].name, parsed[1].outbound, parsed[1].shareLink, metadata = parsed[1].metadata)
        val base = AppData(nodes = listOf(vlessNode, hy2Node), groups = listOf(group), settings = mapOf("selectedNodeId" to "10", "selectedGroupId" to "1"))

        // sing-box default does not invoke extra core
        val sbCfg = ConfigBuilder.build(base, Purpose.MAIN)
        assertEquals("sing-box", MultiCorePlanner.extractCoreKind(sbCfg))
        assertNull(MultiCorePlanner.plan(sbCfg))

        // Xray test build on VLESS Reality + XHTTP
        val xrayData = base.copy(settings = base.settings + ("proxyCore" to "xray"))
        val xrayCfg = ConfigBuilder.build(xrayData, Purpose.TEST, testNodeId = 10)
        assertEquals("xray", MultiCorePlanner.extractCoreKind(xrayCfg))
        val xrayPlan = MultiCorePlanner.plan(xrayCfg)
        assertNotNull(xrayPlan)
        assertEquals("xray", xrayPlan!!.kind)
        assertEquals(2, xrayPlan.ports.size)
        val xrayExtra = JSONObject(xrayPlan.extraCoreConfig)
        assertTrue(xrayExtra.getString("dnsServer").startsWith("127.0.0.1:"))
        val xOut = xrayExtra.getJSONObject("coreConfig").getJSONArray("outbounds").getJSONObject(0)
        assertEquals("vless", xOut.getString("protocol"))
        assertEquals("xhttp", xOut.getJSONObject("streamSettings").getString("network"))
        assertEquals("reality", xOut.getJSONObject("streamSettings").getString("security"))
        assertEquals(1, xOut.getJSONObject("mux").getInt("xudpConcurrency"))

        // EXPORT must never leak links_experimental_core
        val exported = ConfigBuilder.build(xrayData, Purpose.EXPORT)
        assertFalse(JSONObject(exported).has("links_experimental_core"))

        // mihomo test build on Hysteria2
        val mihomoData = base.copy(settings = base.settings + ("proxyCore" to "mihomo"))
        val mihomoCfg = ConfigBuilder.build(mihomoData, Purpose.TEST, testNodeId = 11)
        val mihomoPlan = MultiCorePlanner.plan(mihomoCfg)
        assertNotNull(mihomoPlan)
        assertEquals("mihomo", mihomoPlan!!.kind)
        val mExtra = JSONObject(mihomoPlan.extraCoreConfig).getJSONObject("coreConfig")
        assertEquals("hysteria2", mExtra.getJSONArray("proxies").getJSONObject(0).getString("type"))
        assertEquals(1, mExtra.getJSONArray("listeners").length())

        // Xray rejects Hysteria2 clearly
        val badXray = ConfigBuilder.build(xrayData, Purpose.TEST, testNodeId = 11)
        try {
            MultiCorePlanner.plan(badXray)
            fail("Expected Xray to reject Hysteria2")
        } catch (e: IllegalArgumentException) {
            assertTrue(e.message.orEmpty().contains("hysteria2"))
        }
    }

    @Test fun rustParserAndPlannerHandleEdgeCasesCaughtInReview() {
        // 1. Base64-decoded JSON with leading whitespace/newline
        val paddedJson = "\n  {\"type\":\"socks\",\"server\":\"1.2.3.4\",\"server_port\":1080,\"tag\":\"ws-json\"}\n"
        val b64 = java.util.Base64.getEncoder().encodeToString(paddedJson.toByteArray(Charsets.UTF_8))
        val parsedB64 = SubscriptionParser.parse(b64).single()
        assertEquals("ws-json", parsedB64.name)

        // 2. Clash JSON proxy with standard "port" key (not "server-port")
        val clashJson = """{"proxies":[{"name":"clash-ss","type":"ss","server":"5.6.7.8","port":8388,"cipher":"aes-256-gcm","password":"pw"}]}"""
        val parsedClash = SubscriptionParser.parse(clashJson).single()
        val clashOut = JSONObject(parsedClash.outbound)
        assertEquals("shadowsocks", clashOut.getString("type"))
        assertEquals(8388, clashOut.getInt("server_port"))
        assertEquals("aes-256-gcm", clashOut.getString("method"))

        // 3. Reject out-of-range port & blank server in Clash and VMess
        val badClashPort = """{"proxies":[{"name":"bad","type":"trojan","server":"a.test","port":70000,"password":"pw"}]}"""
        assertTrue(runCatching { SubscriptionParser.parse(badClashPort) }.isFailure)
        val badClashServer = """{"proxies":[{"name":"bad","type":"trojan","server":"","port":443,"password":"pw"}]}"""
        assertTrue(runCatching { SubscriptionParser.parse(badClashServer) }.isFailure)

        // 4. MultiCore TLS disabled-state validation: {} ok, {"server_name":"x"} without enabled rejected
        val strayTls = """{"links_experimental_core":"xray","outbounds":[{"type":"vless","tag":"n1","server":"1.2.3.4","server_port":443,"uuid":"11111111-1111-1111-1111-111111111111","tls":{"server_name":"a.com"}},{"type":"direct","tag":"direct"}],"route":{"rules":[],"final":"n1","default_domain_resolver":"dns-direct"},"dns":{"servers":[{"type":"local","tag":"dns-direct"}],"rules":[]}}"""
        assertTrue(runCatching { MultiCorePlanner.plan(strayTls) }.isFailure)
        val emptyTls = strayTls.replace("""{"server_name":"a.com"}""", "{}")
        assertNotNull(MultiCorePlanner.plan(emptyTls))
    }
}
