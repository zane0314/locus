package com.zane.zanebox.core

import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.IBinder
import android.os.ParcelFileDescriptor

/**
 * Hosts `libextra_cores.so` (Xray / mihomo) in an isolated process (`:extraMain` or `:extraTest`)
 * so each extra kernel has clean process-level lifecycle isolation and cannot disrupt `:bg`.
 */
abstract class ExtraCoreService : Service() {
    private val lock = Any()

    private fun checkCallerUid() {
        if (Binder.getCallingUid() != applicationInfo.uid) {
            throw SecurityException("仅允许应用自身进程调用外核服务")
        }
    }

    private val binder = object : IExtraCore.Stub() {
        override fun validate(kind: String, config: String): String {
            checkCallerUid()
            return synchronized(lock) { ExtraCoreNative.validate(kind, config) }
        }

        override fun start(kind: String, config: String, protector: IExtraCoreProtect?): String {
            checkCallerUid()
            val bridge = protector?.let { remote ->
                ExtraCoreProtect { fd ->
                    try {
                        ParcelFileDescriptor.fromFd(fd).use { pfd -> remote.protect(pfd) }
                    } catch (_: Throwable) {
                        false
                    }
                }
            }
            return synchronized(lock) { ExtraCoreNative.start(kind, config, bridge) }
        }

        override fun stop(): String {
            checkCallerUid()
            return synchronized(lock) { ExtraCoreNative.stop() }
        }

        override fun version(kind: String): String {
            checkCallerUid()
            return synchronized(lock) { ExtraCoreNative.version(kind) }
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onDestroy() {
        synchronized(lock) {
            runCatching { ExtraCoreNative.stop() }
        }
        super.onDestroy()
    }
}

class ExtraCoreMainService : ExtraCoreService()
class ExtraCoreTestService : ExtraCoreService()
