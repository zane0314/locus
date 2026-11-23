#!/usr/bin/env python3
"""Build and install minimal launchable stub APKs required by AppPickerTest and AppFeatureTest."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import zipfile

PACKAGES = [
    *(f"com.google.android.linksfixture.p{i}" for i in range(10)),
    "com.example.linkslocal.one",
    "com.example.linkslocal.two",
]


def find_sdk_root() -> Path:
    candidates = [
        os.environ.get("ANDROID_SDK_ROOT"),
        os.environ.get("ANDROID_HOME"),
        "/opt/homebrew/share/android-commandlinetools",
        str(Path.home() / "Library/Android/sdk"),
    ]
    for raw in candidates:
        if raw:
            p = Path(raw)
            if (p / "build-tools").is_dir() and (p / "platforms").is_dir():
                return p
    raise RuntimeError("No Android SDK with build-tools and platforms found")


def find_build_tools() -> Path:
    bt_root = find_sdk_root() / "build-tools"
    versions = sorted((p for p in bt_root.iterdir() if p.is_dir()), reverse=True)
    if not versions:
        raise RuntimeError(f"No Android build-tools under {bt_root}")
    return versions[0]


def find_android_jar() -> Path:
    sdk = find_sdk_root()
    platforms = sorted((p for p in (sdk / "platforms").iterdir() if (p / "android.jar").is_file()), reverse=True)
    if not platforms:
        raise RuntimeError(f"No android.jar under {sdk / 'platforms'}")
    return platforms[0] / "android.jar"


def ensure_debug_keystore(work: Path) -> Path:
    ks = work / "fixture-debug.jks"
    if not ks.is_file():
        subprocess.run(
            [
                "keytool", "-genkeypair", "-v",
                "-keystore", str(ks),
                "-storepass", "android",
                "-alias", "androiddebugkey",
                "-keypass", "android",
                "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000",
                "-dname", "CN=Android Debug,O=Android,C=US",
            ],
            check=True,
            capture_output=True,
        )
    return ks


def build_and_install(adb: str, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    installed = subprocess.check_output([adb, "shell", "pm", "list", "packages"], text=True)
    installed_set = {
        line.strip().removeprefix("package:")
        for line in installed.splitlines()
        if line.strip().startswith("package:")
    }
    missing = [pkg for pkg in PACKAGES if pkg not in installed_set]
    if not missing:
        return

    bt = find_build_tools()
    aapt2 = bt / "aapt2"
    d8 = bt / "d8"
    apksigner = bt / "apksigner"
    android_jar = find_android_jar()
    ks = ensure_debug_keystore(out_dir)

    # Compile a tiny shared android.app.Activity subclass once, or use android.app.Activity directly!
    # An APK with hasCode="false" and targetActivity/Activity android:name="android.app.Activity" needs zero DEX!
    for idx, pkg in enumerate(missing):
        pkg_dir = out_dir / pkg
        if pkg_dir.exists():
            shutil.rmtree(pkg_dir)
        pkg_dir.mkdir(parents=True)
        label = f"Fixture {pkg.rsplit('.', 1)[-1].upper()}"
        manifest = pkg_dir / "AndroidManifest.xml"
        manifest.write_text(
            f'''<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{pkg}"
    android:versionCode="1"
    android:versionName="1.0">
    <uses-sdk android:minSdkVersion="26" android:targetSdkVersion="35"/>
    <application android:label="{label}" android:hasCode="false">
        <activity android:name="android.app.Activity" android:exported="true" android:label="{label}">
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity>
    </application>
</manifest>
''',
            encoding="utf-8",
        )
        unsigned_apk = pkg_dir / "unsigned.apk"
        signed_apk = out_dir / f"{pkg}.apk"
        subprocess.run(
            [
                str(aapt2), "link",
                "-I", str(android_jar),
                "--manifest", str(manifest),
                "-o", str(unsigned_apk),
            ],
            check=True,
            capture_output=True,
        )
        # Align / sign with apksigner (v1+v2+v3)
        zipalign = bt / "zipalign"
        aligned_apk = pkg_dir / "aligned.apk"
        subprocess.run([str(zipalign), "-f", "4", str(unsigned_apk), str(aligned_apk)], check=True, capture_output=True)
        subprocess.run(
            [
                str(apksigner), "sign",
                "--ks", str(ks),
                "--ks-pass", "pass:android",
                "--key-pass", "pass:android",
                "--ks-key-alias", "androiddebugkey",
                "--out", str(signed_apk),
                str(aligned_apk),
            ],
            check=True,
            capture_output=True,
        )
        subprocess.run([adb, "install", "-r", str(signed_apk)], check=True, capture_output=True)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--adb", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    build_and_install(args.adb, Path(args.out))


if __name__ == "__main__":
    main()
