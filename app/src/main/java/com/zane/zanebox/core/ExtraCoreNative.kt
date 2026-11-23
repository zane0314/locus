package com.zane.zanebox.core

/**
 * Direct JNI bridge into `libextra_cores.so` (Xray-core + mihomo).
 * Package & symbol signatures are ABI-pinned to `Java_com_zane_zanebox_core_ExtraCoreNative_*`.
 */
object ExtraCoreNative {
    @Volatile
    private var loaded = false

    @Synchronized
    private fun ensureLoaded() {
        if (!loaded) {
            System.loadLibrary("extra_cores")
            loaded = true
        }
    }

    @JvmStatic
    private external fun nativeStart(kind: ByteArray, config: ByteArray, protector: ExtraCoreProtect?): ByteArray

    @JvmStatic
    private external fun nativeStop(): ByteArray

    @JvmStatic
    private external fun nativeValidate(kind: ByteArray, config: ByteArray): ByteArray

    @JvmStatic
    private external fun nativeVersion(kind: ByteArray): ByteArray

    fun start(kind: String, config: String, protector: ExtraCoreProtect?): String {
        ensureLoaded()
        return nativeStart(kind.toByteArray(Charsets.UTF_8), config.toByteArray(Charsets.UTF_8), protector)
            .toString(Charsets.UTF_8)
    }

    fun stop(): String {
        ensureLoaded()
        return nativeStop().toString(Charsets.UTF_8)
    }

    fun validate(kind: String, config: String): String {
        ensureLoaded()
        return nativeValidate(kind.toByteArray(Charsets.UTF_8), config.toByteArray(Charsets.UTF_8))
            .toString(Charsets.UTF_8)
    }

    fun version(kind: String): String {
        ensureLoaded()
        return nativeVersion(kind.toByteArray(Charsets.UTF_8)).toString(Charsets.UTF_8)
    }
}
