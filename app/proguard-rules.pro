# go.** and libcore.** are already retained by the AAR's consumer rules.
# Native callbacks call these public methods through Go/Seq and JNI.
-keep class com.zane.zanebox.core.NativePlatform { public *; }
-keep class com.zane.zanebox.core.StringValues { public *; }
-keep class com.zane.zanebox.core.RustCore { *; }
-keep class com.zane.zanebox.core.ExtraCoreNative { *; }
-keep interface com.zane.zanebox.core.ExtraCoreProtect { *; }
