package com.zane.zanebox.core

import android.content.Context
import kotlinx.coroutines.CancellationException
import org.json.JSONObject

data class ExtraCorePlan(
    val kind: String,
    val singBoxConfig: String,
    val extraCoreConfig: String,
    val ports: List<Int>,
)

object MultiCorePlanner {
    const val CORE_SING_BOX = "sing-box"
    const val CORE_XRAY = "xray"
    const val CORE_MIHOMO = "mihomo"

    fun extractCoreKind(config: String): String =
        RustCore.callString("extractCoreKind", JSONObject().put("config", config))

    fun withCoreKind(config: String, coreKind: String): String =
        RustCore.callString("withCoreKind", JSONObject().put("config", config).put("coreKind", coreKind))

    fun plan(config: String): ExtraCorePlan? {
        val obj = RustCore.callNullableObject("planExtraCore", JSONObject().put("config", config)) ?: return null
        val portsArr = obj.getJSONArray("ports")
        val ports = (0 until portsArr.length()).map { portsArr.getInt(it) }
        return ExtraCorePlan(
            kind = obj.getString("kind"),
            singBoxConfig = obj.getString("singBoxConfig"),
            extraCoreConfig = obj.getString("extraCoreConfig"),
            ports = ports,
        )
    }
}

/**
 * Coordinates the three proxy cores (`sing-box`, `xray`, `mihomo`):
 * - `sing-box` runs directly in `SingBoxEngine`.
 * - `xray` / `mihomo` run in isolated `:extraMain` / `:extraTest` processes via `libextra_cores.so`,
 *   bridged to sing-box's TUN/routing/DNS stack through Rust-planned loopback SOCKS5 + DNS bridges.
 */
class MultiCoreEngine(
    context: Context,
    private val platform: NativePlatform,
    private val protectFd: ((Int) -> Boolean)? = null,
) {
    private val singBox = SingBoxEngine(platform)
    private val extraMain = ExtraCoreClient(context.applicationContext, ExtraCoreMainService::class.java, protectFd)
    private val extraTest = ExtraCoreClient(context.applicationContext, ExtraCoreTestService::class.java, protectFd)
    private val extraTestLock = Any()

    @Volatile
    private var activeExtraKind: String? = null

    val trackedTags: List<String> get() = singBox.trackedTags
    val configuredProxyDefault: String get() = singBox.configuredProxyDefault

    fun start(config: String) {
        val plan = MultiCorePlanner.plan(config)
        if (plan == null) {
            singBox.start(config)
            activeExtraKind = null
            return
        }
        try {
            extraMain.validate(plan.kind, plan.extraCoreConfig)
            singBox.start(plan.singBoxConfig)
            extraMain.start(plan.kind, plan.extraCoreConfig)
            activeExtraKind = plan.kind
        } catch (failure: Throwable) {
            runCatching { extraMain.close() }
            runCatching { singBox.close() }
            activeExtraKind = null
            throw failure
        }
    }

    fun close(): Map<String, Pair<Long, Long>> {
        val kind = activeExtraKind
        activeExtraKind = null
        try {
            if (kind != null) runCatching { extraMain.stop() }
            return singBox.close()
        } finally {
            runCatching { extraMain.close() }
            runCatching { extraTest.close() }
        }
    }

    fun query(tag: String, direction: String): Long = singBox.query(tag, direction)
    fun select(tag: String): Boolean = singBox.select(tag)
    fun selectGroup(groupTag: String, tag: String): Boolean = singBox.selectGroup(groupTag, tag)
    fun applyURLTestResults(groupTag: String, results: Map<String, Int>): String =
        singBox.applyURLTestResults(groupTag, results)

    fun resetNetwork() = singBox.resetNetwork()
    fun sleep() = singBox.sleep()
    fun wake() = singBox.wake()

    fun validate(config: String) {
        val plan = MultiCorePlanner.plan(config)
        if (plan == null) {
            singBox.validate(config)
            return
        }
        singBox.validate(plan.singBoxConfig)
        synchronized(extraTestLock) {
            try {
                extraTest.validate(plan.kind, plan.extraCoreConfig)
            } finally {
                extraTest.close()
            }
        }
    }

    /**
     * Runs a speed-test block with a clean sing-box config (and an active extra-core test process when
     * `proxyCore` is `xray` or `mihomo`), guaranteeing cleanup of `:extraTest` when the block ends.
     */
    suspend fun <T> withSpeedTestConfig(
        config: String,
        url: String,
        timeout: Int,
        active: () -> Boolean = { true },
        block: suspend (String) -> T,
    ): T {
        val plan = MultiCorePlanner.plan(config)
        if (plan == null) {
            val resolved = singBox.resolvedTestConfig(config, url, timeout, active)
            return block(resolved)
        }
        if (!active()) throw CancellationException("测速已取消")
        val singBoxCfg = synchronized(extraTestLock) {
            if (!active()) throw CancellationException("测速已取消")
            extraTest.validate(plan.kind, plan.extraCoreConfig)
            extraTest.start(plan.kind, plan.extraCoreConfig)
            try {
                singBox.resolvedTestConfig(plan.singBoxConfig, url, timeout, active)
            } catch (e: Throwable) {
                runCatching { extraTest.close() }
                throw e
            }
        }
        return try {
            block(singBoxCfg)
        } finally {
            runCatching { extraTest.close() }
        }
    }

    fun test(
        config: String,
        url: String,
        timeout: Int,
        prepared: (String) -> Unit = {},
        frontDelays: MutableMap<String, Int>? = null,
        active: () -> Boolean = { true },
    ): Int {
        val plan = MultiCorePlanner.plan(config)
        if (plan == null) {
            return singBox.test(config, url, timeout, prepared, frontDelays, active)
        }
        if (!active()) throw CancellationException("测速已取消")
        return synchronized(extraTestLock) {
            if (!active()) throw CancellationException("测速已取消")
            try {
                extraTest.validate(plan.kind, plan.extraCoreConfig)
                if (!active()) throw CancellationException("测速已取消")
                extraTest.start(plan.kind, plan.extraCoreConfig)
                if (!active()) throw CancellationException("测速已取消")
                singBox.test(plan.singBoxConfig, url, timeout, prepared, frontDelays, active)
            } finally {
                runCatching { extraTest.stop() }
            }
        }
    }

    fun cancelTests() {
        singBox.cancelTests()
        runCatching { extraTest.close() }
    }
}
