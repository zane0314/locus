package com.zane.zanebox.core

/** JNI callback interface expected by `libextra_cores.so` (package & name must stay exact). */
fun interface ExtraCoreProtect {
    fun protect(fd: Int): Boolean
}
