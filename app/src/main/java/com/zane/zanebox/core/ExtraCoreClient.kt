package com.zane.zanebox.core

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import android.os.ParcelFileDescriptor
import java.io.Closeable
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/**
 * Bound AIDL client for `:extraMain` / `:extraTest` extra-kernel processes.
 * Uses separate bind serialization (`bindLock`) and fast state synchronization (`stateLock`)
 * so Main-Thread ServiceConnection callbacks never deadlock against `latch.await()`.
 */
class ExtraCoreClient(
    private val context: Context,
    private val serviceClass: Class<out ExtraCoreService>,
    private val protectFd: ((Int) -> Boolean)? = null,
) : Closeable {
    private val bindLock = Any()
    private val stateLock = Any()
    @Volatile
    private var service: IExtraCore? = null
    private var connection: ServiceConnection? = null

    private fun ensureBound(): IExtraCore {
        service?.let { return it }
        synchronized(bindLock) {
            service?.let { return it }
            val latch = CountDownLatch(1)
            val boundRef = AtomicReference<IExtraCore?>(null)
            lateinit var conn: ServiceConnection
            conn = object : ServiceConnection {
                override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                    val iface = IExtraCore.Stub.asInterface(binder)
                    synchronized(stateLock) {
                        if (connection === conn) {
                            service = iface
                        }
                    }
                    boundRef.set(iface)
                    latch.countDown()
                }

                override fun onServiceDisconnected(name: ComponentName?) {
                    detachConnection(conn)
                    latch.countDown()
                }

                override fun onBindingDied(name: ComponentName?) {
                    detachConnection(conn)
                    latch.countDown()
                }
            }
            val staleConn = synchronized(stateLock) {
                val prev = connection
                connection = conn
                service = null
                prev
            }
            if (staleConn != null) {
                runCatching { context.unbindService(staleConn) }
            }
            val intent = Intent(context, serviceClass)
            val ok = context.bindService(intent, conn, Context.BIND_AUTO_CREATE or Context.BIND_IMPORTANT)
            if (!ok) {
                detachConnection(conn)
                error("无法绑定外核服务 ${serviceClass.simpleName}")
            }
            if (!latch.await(10, TimeUnit.SECONDS)) {
                detachConnection(conn)
                error("连接外核服务超时")
            }
            return boundRef.get() ?: run {
                detachConnection(conn)
                error("外核服务连接失败")
            }
        }
    }

    private fun detachConnection(target: ServiceConnection) {
        val shouldUnbind = synchronized(stateLock) {
            if (connection === target) {
                connection = null
                service = null
                true
            } else {
                false
            }
        }
        if (shouldUnbind) {
            runCatching { context.unbindService(target) }
        }
    }

    fun validate(kind: String, config: String) {
        val err = ensureBound().validate(kind, config)
        if (err.isNotBlank()) throw IllegalArgumentException(err)
    }

    fun start(kind: String, config: String) {
        val protector = protectFd?.let { cb ->
            object : IExtraCoreProtect.Stub() {
                override fun protect(fd: ParcelFileDescriptor?): Boolean {
                    val pfd = fd ?: return false
                    return pfd.use { cb(it.fd) }
                }
            }
        }
        val err = ensureBound().start(kind, config, protector)
        if (err.isNotBlank()) throw IllegalStateException(err)
    }

    fun stop() {
        val svc = service ?: return
        runCatching { svc.stop() }
    }

    fun version(kind: String): String = runCatching { ensureBound().version(kind) }.getOrDefault("")

    override fun close() {
        val (svc, conn) = synchronized(stateLock) {
            val s = service
            val c = connection
            service = null
            connection = null
            s to c
        }
        if (svc != null) runCatching { svc.stop() }
        if (conn != null) runCatching { context.unbindService(conn) }
    }
}
