package com.zane.zanebox.core

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Binder
import android.os.DeadObjectException
import android.os.IBinder
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.os.RemoteException
import android.util.Log
import java.io.Closeable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong

/**
 * Bound AIDL client for `:extraMain` / `:extraTest` extra-kernel processes.
 * Enforces off-main-thread connection, clears calling identity on FD protection,
 * and waits for process exit on `close()` so consecutive runs always get a fresh process.
 */
class ExtraCoreClient(
    private val context: Context,
    private val serviceClass: Class<out ExtraCoreService>,
    private val protectFd: ((Int) -> Boolean)? = null,
    private val onUnexpectedLost: (() -> Unit)? = null,
) : Closeable {
    private val generation = AtomicLong()
    @Volatile
    private var service: IExtraCore? = null
    @Volatile
    private var closing = false
    private var connection: ServiceConnection? = null
    private var deathLatch: CountDownLatch? = null

    private val protectorStub = protectFd?.let { cb ->
        object : IExtraCoreProtect.Stub() {
            override fun protect(fd: ParcelFileDescriptor?): Boolean {
                val pfd = fd ?: return false
                val caller = Binder.clearCallingIdentity()
                return try {
                    pfd.use { cb(it.fd) }
                } catch (e: Exception) {
                    Log.w("LinksExtraCore", "附加内核套接字保护失败", e)
                    false
                } finally {
                    Binder.restoreCallingIdentity(caller)
                }
            }
        }
    }

    private fun ensureBound(): IExtraCore {
        service?.let { return it }
        check(Looper.myLooper() != Looper.getMainLooper()) { "附加内核必须在后台线程连接" }
        if (connection != null) {
            close()
        }
        closing = false
        val gen = generation.incrementAndGet()
        val connectLatch = CountDownLatch(1)
        val procDeathLatch = CountDownLatch(1)
        deathLatch = procDeathLatch
        val conn = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                try {
                    if (binder != null && !closing && generation.get() == gen) {
                        binder.linkToDeath({
                            procDeathLatch.countDown()
                            if (generation.get() == gen) {
                                service = null
                                if (!closing) {
                                    onUnexpectedLost?.invoke()
                                }
                            }
                        }, 0)
                        if (binder.isBinderAlive) {
                            service = IExtraCore.Stub.asInterface(binder)
                        }
                    }
                } catch (_: RemoteException) {
                    procDeathLatch.countDown()
                } finally {
                    connectLatch.countDown()
                }
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                if (generation.get() == gen) {
                    service = null
                }
                connectLatch.countDown()
            }

            override fun onBindingDied(name: ComponentName?) {
                onServiceDisconnected(name)
            }

            override fun onNullBinding(name: ComponentName?) {
                connectLatch.countDown()
            }
        }
        connection = conn
        try {
            val intent = Intent(context, serviceClass)
            if (!context.bindService(intent, conn, Context.BIND_AUTO_CREATE)) {
                error("附加内核服务连接失败")
            }
            if (!connectLatch.await(15, TimeUnit.SECONDS)) {
                error("附加内核服务连接超时")
            }
            return service ?: error("附加内核服务不可用")
        } catch (t: Throwable) {
            close()
            throw t
        }
    }

    @Synchronized
    fun validate(kind: String, config: String) {
        val err = ensureBound().validate(kind, config)
        if (err.isNotBlank()) throw IllegalArgumentException("$kind 配置校验失败：$err")
    }

    @Synchronized
    fun start(kind: String, config: String) {
        val err = ensureBound().start(kind, config, protectorStub)
        if (err.isNotBlank()) throw IllegalStateException("$kind 启动失败：$err")
    }

    fun stop() {
        val svc = service ?: return
        runCatching { svc.stop() }
    }

    @Synchronized
    fun version(kind: String): String = runCatching { ensureBound().version(kind) }.getOrDefault("")

    @Synchronized
    override fun close() {
        closing = true
        generation.incrementAndGet()
        val svc = service
        service = null
        val waitDeath = deathLatch
        val conn = connection
        connection = null
        deathLatch = null
        try {
            if (svc != null) {
                try {
                    svc.stop()
                } catch (_: DeadObjectException) {
                }
            }
        } finally {
            if (conn != null) {
                runCatching { context.unbindService(conn) }
            }
            if (svc != null && waitDeath != null) {
                if (!waitDeath.await(5, TimeUnit.SECONDS)) {
                    error("附加内核进程退出超时")
                }
            }
        }
    }
}
