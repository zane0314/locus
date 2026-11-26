package com.zane.zanebox

import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.zane.zanebox.config.ConfigBuilder
import com.zane.zanebox.config.Purpose
import com.zane.zanebox.core.MultiCorePlanner
import com.zane.zanebox.core.NativePlatform
import com.zane.zanebox.core.SingBoxEngine
import com.zane.zanebox.data.AppData
import com.zane.zanebox.data.Group
import com.zane.zanebox.data.Node
import com.zane.zanebox.runtime.ServiceClient
import com.zane.zanebox.subscription.SubscriptionParser
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.BufferedInputStream
import java.io.File
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicInteger
import kotlin.concurrent.thread

@RunWith(AndroidJUnit4::class)
class Locus107AcceptanceTest {
    @Test fun hysteria2GeckoConfigsValidateInRealLibcoreAndMihomoPlanner() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val samples = listOf(
            "hysteria2://secret-auth@198.51.100.10:443/?sni=edge.example.test&alpn=h3&insecure=0&obfs=gecko&obfs-password=gecko-pass#Gecko-IPv4",
            "hy2://secret-auth@[2001:db8::10]:443?sni=edge6.example.test&alpn=h3&obfs=gecko&obfs-password=gecko-pass-2&obfs-min-packet-size=600&obfs-max-packet-size=1100#Gecko-IPv6-Sized",
            """
            proxies:
              - name: Clash-Gecko
                type: hysteria2
                server: 198.51.100.12
                port: 8443
                password: pw
                sni: clash.example.test
                obfs: gecko
                obfs-password: clash-gecko-pass
                obfs-min-packet-size: 512
                obfs-max-packet-size: 1200
            """.trimIndent(),
        )
        val platform = NativePlatform(context, null) { _, _ -> }
        platform.initialize()
        val engine = SingBoxEngine(platform)
        val client = ServiceClient(context)
        client.connect()
        val evidence = JSONObject()
        try {
            samples.forEachIndexed { index, sample ->
                val parsed = SubscriptionParser.parse(sample).single()
                val obfs = JSONObject(parsed.outbound).getJSONObject("obfs")
                assertEquals("gecko", obfs.getString("type"))
                assertTrue(obfs.getString("password").isNotEmpty())

                val data = AppData(
                    nodes = listOf(Node(1, 1, parsed.name, parsed.outbound)),
                    groups = listOf(Group(1, "GeckoFixture")),
                    settings = mapOf("selectedNodeId" to "1", "selectedGroupId" to "1"),
                )
                val cfg = ConfigBuilder.build(data, Purpose.TEST, 1)
                engine.validate(cfg)

                val key = "gecko-validate-$index"
                client.validateConfig(key, cfg)
                val deadline = SystemClock.elapsedRealtime() + 15000
                while (key !in client.validationResults.value && SystemClock.elapsedRealtime() < deadline) Thread.sleep(20)
                assertTrue("原生校验未返回 $key", key in client.validationResults.value)
                assertEquals("原生拒绝 gecko 配置 $key", "", client.validationResults.value[key])

                val mihomoCfg = ConfigBuilder.build(data.copy(settings = data.settings + ("proxyCore" to "mihomo")), Purpose.TEST, 1)
                val mihomoPlan = MultiCorePlanner.plan(mihomoCfg)
                assertNotNull("mihomo planner must accept hysteria2 gecko", mihomoPlan)
                val mProxy = JSONObject(mihomoPlan!!.extraCoreConfig).getJSONObject("coreConfig").getJSONArray("proxies").getJSONObject(0)
                assertEquals("gecko", mProxy.getString("obfs"))
                evidence.put("sample_$index", JSONObject().put("name", parsed.name).put("libcoreValid", true).put("mihomoObfs", mProxy.getString("obfs")))
            }
            File(context.getExternalFilesDir(null), "gecko-validation.json").writeText(evidence.toString(2))
        } finally {
            client.close()
            platform.close()
        }
    }

    private fun readHttpHeader(input: BufferedInputStream): String? {
        val buf = StringBuilder()
        var matched = 0
        while (true) {
            val b = input.read()
            if (b == -1) return if (buf.isEmpty()) null else buf.toString()
            val c = b.toChar()
            buf.append(c)
            matched = when {
                matched == 0 && c == '\r' -> 1
                matched == 1 && c == '\n' -> 2
                matched == 2 && c == '\r' -> 3
                matched == 3 && c == '\n' -> 4
                c == '\r' -> 1
                else -> 0
            }
            if (matched == 4) return buf.toString()
        }
    }

    private fun startHttpProxyFixture(mode: String, connections: AtomicInteger): ServerSocket {
        val server = ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"))
        thread(isDaemon = true, name = "unified-fixture-$mode") {
            while (!server.isClosed) {
                val sock: Socket = try { server.accept() } catch (_: Exception) { break }
                val connNo = connections.incrementAndGet()
                thread(isDaemon = true) {
                    sock.use { s ->
                        s.soTimeout = 5000
                        val input = BufferedInputStream(s.getInputStream())
                        val out: OutputStream = s.getOutputStream()
                        val connectReq = readHttpHeader(input) ?: return@use
                        if (!connectReq.startsWith("CONNECT ")) return@use
                        Thread.sleep(360)
                        out.write("HTTP/1.1 200 Connection Established\r\n\r\n".toByteArray())
                        out.flush()

                        // First HTTP request on the tunneled connection
                        if (readHttpHeader(input) == null) return@use
                        when (mode) {
                            "reuse_ok" -> {
                                out.write("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n".toByteArray())
                                out.flush()
                                if (readHttpHeader(input) != null) {
                                    out.write("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray())
                                    out.flush()
                                }
                            }
                            "force_redial" -> {
                                // Advertise keep-alive but close the socket immediately afterwards to simulate
                                // a proxy/middlebox dropping an idle keep-alive connection and forcing a cold redial.
                                out.write("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n".toByteArray())
                                out.flush()
                            }
                            "explicit_close" -> {
                                out.write("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray())
                                out.flush()
                            }
                            "second_timeout" -> {
                                out.write("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n".toByteArray())
                                out.flush()
                                if (readHttpHeader(input) != null) {
                                    Thread.sleep(3000)
                                }
                            }
                        }
                    }
                }
            }
        }
        return server
    }

    @Test fun unifiedDelayRejectsRedialAndSecondTimeoutWithoutColdFallback() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val platform = NativePlatform(context, null) { _, _ -> }
        platform.initialize()
        val engine = SingBoxEngine(platform)
        val evidence = JSONObject()
        try {
            fun runCase(mode: String, timeoutMs: Int): Pair<Int?, Int> {
                val conns = AtomicInteger(0)
                val server = startHttpProxyFixture(mode, conns)
                try {
                    val data = AppData(
                        nodes = listOf(Node(1, 1, "HTTP-$mode", """{"type":"http","server":"127.0.0.1","server_port":${server.localPort}}""")),
                        groups = listOf(Group(1, "UnifiedFixture")),
                        settings = mapOf("unifiedDelay" to "true", "tcpConcurrent" to "true", "dnsRemote" to "local"),
                    )
                    val cfg = ConfigBuilder.build(data, Purpose.TEST, 1)
                    val latency = runCatching { engine.test(cfg, "http://203.0.113.10/measure", timeoutMs, singleAttempt = true) }.getOrNull()
                    return latency to conns.get()
                } finally {
                    runCatching { server.close() }
                }
            }

            val (reuseLatency, reuseConns) = runCase("reuse_ok", 3000)
            assertNotNull("reused unified probe must succeed", reuseLatency)
            assertTrue("reused unified latency must exclude 360ms CONNECT handshake: $reuseLatency", reuseLatency!! in 1..250)
            assertEquals("reused probe must use 1 TCP connection", 1, reuseConns)

            val (redialLatency, redialConns) = runCase("force_redial", 3000)
            assertNull("unifiedDelay must reject non-reused 2nd request instead of returning cold latency ($redialLatency)", redialLatency)
            assertTrue("force_redial should have observed 2nd dial: $redialConns", redialConns >= 2)

            val (timeoutLatency, timeoutConns) = runCase("second_timeout", 1100)
            assertNull("unifiedDelay must fail when 2nd probe times out instead of returning cold latency ($timeoutLatency)", timeoutLatency)
            assertEquals(1, timeoutConns)

            val (closeLatency, closeConns) = runCase("explicit_close", 3000)
            assertNotNull("explicit Connection: close endpoint must not fail", closeLatency)
            assertTrue("explicit Connection: close returns 1st probe latency (>=350ms): $closeLatency", closeLatency!! >= 350)
            assertEquals("explicit Connection: close must skip wasteful 2nd dial", 1, closeConns)

            evidence
                .put("reuseOkLatencyMs", reuseLatency)
                .put("reuseOkConns", reuseConns)
                .put("forceRedialRejected", redialLatency == null)
                .put("forceRedialConns", redialConns)
                .put("secondTimeoutRejected", timeoutLatency == null)
                .put("explicitCloseLatencyMs", closeLatency)
                .put("explicitCloseConns", closeConns)
            File(context.getExternalFilesDir(null), "unified-delay-107.json").writeText(evidence.toString(2))
        } finally {
            engine.cancelTests()
            platform.close()
        }
    }
}
