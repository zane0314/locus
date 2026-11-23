package com.zane.zanebox.core

import android.app.Service
import android.content.Intent
import android.os.Binder
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.os.Process

/**
 * Hosts `libextra_cores.so` (Xray / mihomo) in an isolated process (`:extraMain` or `:extraTest`)
 * so each extra kernel has clean process-level lifecycle isolation and exits cleanly on stop/unbind.
 */
abstract class ExtraCoreService : Service() {
    private val lock = Any()
    private var started = false
    private var stopping = false
    private var protectorBinder: IBinder? = null
    private val protectorDeath = IBinder.DeathRecipient {
        synchronized(lock) { stopAndExitLocked() }
    }

    private fun checkCallerUid() {
        if (Binder.getCallingUid() != applicationInfo.uid) {
            throw SecurityException("附加内核只接受本应用请求")
        }
    }

    private fun scheduleAsyncExit() {
        val shouldStart = synchronized(lock) {
            if (stopping) false else {
                stopping = true
                true
            }
        }
        if (!shouldStart) return
        Thread({
            try {
                runCatching { ExtraCoreNative.stop() }
                synchronized(lock) {
                    protectorBinder?.let { runCatching { it.unlinkToDeath(protectorDeath, 0) } }
                    protectorBinder = null
                }
            } finally {
                Process.killProcess(Process.myPid())
            }
        }, "extra-core-stop").start()
    }

    private fun stopAndExitLocked(): String {
        if (stopping) return ""
        stopping = true
        return try {
            val err = ExtraCoreNative.stop()
            protectorBinder?.let { runCatching { it.unlinkToDeath(protectorDeath, 0) } }
            protectorBinder = null
            err
        } finally {
            Handler(Looper.getMainLooper()).post { Process.killProcess(Process.myPid()) }
        }
    }

    private val binder = object : IExtraCore.Stub() {
        override fun validate(kind: String, config: String): String {
            checkCallerUid()
            return synchronized(lock) {
                check(!stopping) { "附加内核进程正在退出" }
                ExtraCoreNative.validate(kind, config)
            }
        }

        override fun start(kind: String, config: String, protector: IExtraCoreProtect?): String {
            checkCallerUid()
            return synchronized(lock) {
                check(!started && !stopping) { "附加内核实例只能启动一次" }
                started = true
                protector?.asBinder()?.let { remoteBinder ->
                    remoteBinder.linkToDeath(protectorDeath, 0)
                    protectorBinder = remoteBinder
                }
                val bridge = protector?.let { remote ->
                    ExtraCoreProtect { fd ->
                        try {
                            ParcelFileDescriptor.fromFd(fd).use { pfd -> remote.protect(pfd) }
                        } catch (_: Throwable) {
                            false
                        }
                    }
                } ?: ExtraCoreProtect { true }
                val err = ExtraCoreNative.start(kind, config, bridge)
                if (err.isNotEmpty()) {
                    stopAndExitLocked()
                }
                err
            }
        }

        override fun stop(): String {
            checkCallerUid()
            return synchronized(lock) { stopAndExitLocked() }
        }

        override fun version(kind: String): String {
            checkCallerUid()
            return synchronized(lock) { ExtraCoreNative.version(kind) }
        }
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onUnbind(intent: Intent?): Boolean {
        scheduleAsyncExit()
        stopSelf()
        return false
    }

    override fun onDestroy() {
        try {
            scheduleAsyncExit()
        } finally {
            super.onDestroy()
        }
    }
}

class ExtraCoreMainService : ExtraCoreService()
class ExtraCoreTestService : ExtraCoreService()
