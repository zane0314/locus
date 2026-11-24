#!/usr/bin/env python3
"""Measure release APK Smart Routing scroll performance with the 240-node SQLite fixture on S25+ emulator."""
import argparse
import csv
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET

PKG = "com.zane.locus"
ACTIVITY = f"{PKG}/com.zane.zanebox.MainActivity"
AVD_NAME = "ZaneBox_S25Plus_API35_Acceptance"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--apk", type=Path, required=True, help="Release APK path")
    parser.add_argument("--db", type=Path, required=True, help="240-node SQLite fixture path")
    parser.add_argument("--out", type=Path, required=True, help="Output directory for evidence")
    parser.add_argument("--max-p95-ms", type=int, default=18)
    parser.add_argument("--max-p99-ms", type=int, default=20)
    args = parser.parse_args()

    serial = os.environ.get("ANDROID_SERIAL", "emulator-5580")
    assert serial.startswith("emulator-"), f"Refusing non-emulator serial: {serial}"
    adb_bin = os.environ.get("ADB", "/opt/homebrew/bin/adb")
    adb = [adb_bin, "-s", serial]
    avd = subprocess.check_output(adb + ["emu", "avd", "name"], text=True).splitlines()[0].strip()
    assert avd == AVD_NAME, f"Unexpected AVD: {avd}"

    def run_adb(*cmd: str) -> str:
        return subprocess.check_output(adb + list(cmd), text=True)

    def shell(*cmd: str) -> str:
        return run_adb("shell", *cmd)

    def screencap(path: Path) -> None:
        data = subprocess.check_output(adb + ["exec-out", "screencap", "-p"])
        path.write_bytes(data)

    def dump_xml(path: Path) -> ET.Element:
        shell("uiautomator", "dump", "/sdcard/window_dump.xml")
        xml_text = shell("cat", "/sdcard/window_dump.xml")
        shell("rm", "-f", "/sdcard/window_dump.xml")
        path.write_text(xml_text, encoding="utf-8")
        return ET.fromstring(xml_text)

    def find_bounds(root: ET.Element, resource_id: str) -> tuple[int, int, int, int]:
        for node in root.iter("node"):
            if node.attrib.get("resource-id") == resource_id:
                m = re.match(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", node.attrib.get("bounds", ""))
                if m:
                    return tuple(int(v) for v in m.groups())
        raise RuntimeError(f"Element not found in UI hierarchy: {resource_id}")

    args.out.mkdir(parents=True, exist_ok=True)
    subprocess.run(adb + ["install", "-r", str(args.apk)], check=True, stdout=subprocess.DEVNULL)
    shell("pm", "clear", PKG)
    shell("pm", "grant", PKG, "android.permission.POST_NOTIFICATIONS")

    # Seed the exact 240-node SQLite database into release app storage via adb root.
    for _ in range(5):
        subprocess.run(adb + ["root"], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.5)
        subprocess.run(adb + ["wait-for-device"], check=True)
        if "uid=0" in shell("id"):
            break
    assert "uid=0" in shell("id"), "Failed to obtain adb root"
    uid = shell("dumpsys", "package", PKG)
    uid_match = re.search(r"userId=(\d+)", uid) or re.search(r"appId=(\d+)", uid)
    assert uid_match, "Unable to resolve app UID"
    app_uid = uid_match.group(1)
    db_dir = f"/data/data/{PKG}/databases"
    shell("mkdir", "-p", db_dir)
    subprocess.run(adb + ["push", str(args.db), f"{db_dir}/zanebox.db"], check=True, stdout=subprocess.DEVNULL)
    shell("rm", "-f", f"{db_dir}/zanebox.db-wal", f"{db_dir}/zanebox.db-shm", f"{db_dir}/zanebox.db-journal")
    shell("chown", "-R", f"{app_uid}:{app_uid}", db_dir)
    shell("chmod", "771", db_dir)
    shell("chmod", "660", f"{db_dir}/zanebox.db")

    shell("am", "force-stop", PKG)
    shell("am", "start", "-W", "-n", ACTIVITY)
    # Wait for cold-boot GMS / dex2oat background activity and initial frame settle.
    time.sleep(8)

    width, height = map(int, re.findall(r"(\d+)x(\d+)", shell("wm", "size"))[-1])
    density_match = re.search(r"(\d+)", shell("wm", "density"))
    dpi = int(density_match.group(1)) if density_match else 480
    density = dpi / 160.0

    nodes_xml = dump_xml(args.out / "nodes-initial.xml")
    screencap(args.out / "nodes-initial.png")

    # Tap Smart Routing tab (tab_1).
    x1, y1, x2, y2 = find_bounds(nodes_xml, "tab_1")
    shell("input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
    time.sleep(2.5)

    smart_xml = dump_xml(args.out / "smart-initial.xml")
    screencap(args.out / "smart-initial.png")

    # Measure card heights in dp from UI hierarchy.
    card_heights_dp: dict[str, float] = {}
    for rid in ("smart_card_speed", "smart_card_youtube", "smart_card_telegram"):
        try:
            _, cy1, _, cy2 = find_bounds(smart_xml, rid)
            card_heights_dp[rid] = round((cy2 - cy1) / density, 2)
        except RuntimeError:
            pass

    def swipe(up: bool) -> None:
        x = int(width * 0.5)
        y_low = int(height * 0.78)
        y_high = int(height * 0.24)
        shell("input", "swipe", str(x), str(y_low if up else y_high), str(x), str(y_high if up else y_low), "420")
        time.sleep(0.65)

    # Warm-up pass (scroll through all cards and back so vector glyphs and ART JIT settle),
    # then reset gfxinfo and run 2 down + 2 up swipes matching 1.0.3 baseline.
    for up in (True, True, False, False):
        swipe(up)
    time.sleep(1.5)

    shell("logcat", "-c")
    shell("dumpsys", "gfxinfo", PKG, "reset")
    t0 = time.monotonic()
    for up in (True, True, False, False):
        swipe(up)
    duration_sec = round(time.monotonic() - t0, 2)
    raw = shell("dumpsys", "gfxinfo", PKG, "framestats")
    (args.out / "release-realistic-smart-gfx.txt").write_text(raw, encoding="utf-8")
    screencap(args.out / "smart-after-scroll.png")

    def parse_int(pattern: str) -> int:
        m = re.search(pattern, raw)
        if not m:
            raise RuntimeError(f"Metric missing from gfxinfo: {pattern}")
        return int(m.group(1))

    p50 = parse_int(r"50th percentile:\s*(\d+)ms")
    p90 = parse_int(r"90th percentile:\s*(\d+)ms")
    p95 = parse_int(r"95th percentile:\s*(\d+)ms")
    p99 = parse_int(r"99th percentile:\s*(\d+)ms")
    missed_vsync = parse_int(r"Number Missed Vsync:\s*(\d+)")
    slow_ui = parse_int(r"Number Slow UI thread:\s*(\d+)")
    total_frames = parse_int(r"Total frames rendered:\s*(\d+)")

    frames = []
    header = None
    for line in raw.splitlines():
        if line.startswith("Flags,"):
            header = next(csv.reader([line]))
            continue
        if header and re.match(r"^\d+,", line):
            vals = next(csv.reader([line]))
            row = dict(zip(header, vals))
            if row.get("Flags") != "0":
                continue
            done = int(row["FrameCompleted"])
            vsync = int(row["IntendedVsync"])
            if done <= vsync or done >= 9223372036854775807:
                continue
            frames.append((done - vsync) / 1e6)

    reasons = []
    if total_frames == 0:
        reasons.append("Total frames rendered is 0")
    if p95 > args.max_p95_ms:
        reasons.append(f"gfx 95th {p95}ms > {args.max_p95_ms}ms")
    if p99 > args.max_p99_ms:
        reasons.append(f"gfx 99th {p99}ms > {args.max_p99_ms}ms")
    if missed_vsync != 0:
        reasons.append(f"Number Missed Vsync={missed_vsync} != 0")
    if slow_ui != 0:
        reasons.append(f"Number Slow UI thread={slow_ui} != 0")

    summary = {
        "status": "FAIL" if reasons else "PASS",
        "reasons": reasons,
        "apk": args.apk.name,
        "fixtureDb": str(args.db),
        "screen": f"{width}x{height}",
        "density": density,
        "durationSec": duration_sec,
        "totalFrames": total_frames,
        "sampledFrames": len(frames),
        "gfx50thMs": p50,
        "gfx90thMs": p90,
        "gfx95thMs": p95,
        "gfx99thMs": p99,
        "missedVsync": missed_vsync,
        "slowUiThread": slow_ui,
        "cardHeightsDp": card_heights_dp,
        "thresholds": {
            "maxP95Ms": args.max_p95_ms,
            "maxP99Ms": args.max_p99_ms,
            "missedVsync": 0,
            "slowUiThread": 0,
        },
        "scope": "S25+ emulator release APK with 240-node fixture; relative scroll check only",
    }
    (args.out / "smart-scroll-summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False))
    return 1 if reasons else 0


if __name__ == "__main__":
    sys.exit(main())
