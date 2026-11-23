#!/usr/bin/env bash
# Fresh S25+ wrapper only; all fixtures public and local.
set -Eeuo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
out="$1"
adb=(/opt/homebrew/bin/adb -s "$ANDROID_SERIAL")
mkdir -p "$out"
rm -f "$out/result.json"
full_run=1
if [[ -n "${ZANE_OPT_CLASSES:-}" ]]; then full_run=0; fi
python3 -B "$root/tests/network_fixture.py" --delay-b-ms 250 --log "$out/fixture.jsonl" >"$out/fixture.stderr" 2>&1 &
fixture=$!
trap 'kill "$fixture" 2>/dev/null || true' EXIT
"${adb[@]}" install -r "$root/app/build/outputs/apk/debug/app-debug.apk" >"$out/install.txt"
"${adb[@]}" install -r "$root/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk" >>"$out/install.txt"
"${adb[@]}" install -r "$root/tests/probe/build/outputs/apk/debug/ZaneProbe-debug.apk" >>"$out/install.txt"
"${adb[@]}" shell pm grant com.zane.locus android.permission.POST_NOTIFICATIONS
"${adb[@]}" shell appops set com.zane.locus ACTIVATE_VPN allow || true
"${adb[@]}" shell settings put global captive_portal_mode 0
"${adb[@]}" shell settings put global private_dns_mode off
"${adb[@]}" reverse tcp:19082 tcp:19082
"${adb[@]}" shell find /sdcard/Android/data/com.zane.locus/files/ -type f >"$out/device-files-before.txt" 2>/dev/null || true
"${adb[@]}" logcat -c
classes="${ZANE_OPT_CLASSES:-OptimizationStoreTest,OptimizationRuntimeTest,ZaneStoreV2Test,ZaneStoreConcurrencyTest,NativeSchemaTest,AcceptanceTest,RuntimeRegressionTest,AutomaticApplyTest,SubscriptionBackgroundTest}"
classes=$(python3 -c 'import sys;print(",".join(c if "." in c else "com.zane.zanebox."+c for c in sys.argv[1].split(",")))' "$classes")
set +e
"${adb[@]}" shell am instrument -w -r -e class "$classes" com.zane.locus.test/androidx.test.runner.AndroidJUnitRunner >"$out/instrumentation.txt" 2>&1
rc=$?
set -e
kill "$fixture";wait "$fixture" 2>/dev/null || true
if [[ "$full_run" -eq 1 && "$rc" -eq 0 ]] && grep -q 'OK (' "$out/instrumentation.txt" && ! grep -q 'FAILURES!!!' "$out/instrumentation.txt"; then
    python3 -B "$root/tests/network_fixture.py" --delay-a-ms 250 --log "$out/selection-fixture.jsonl" >"$out/selection-fixture.stderr" 2>&1 &
    fixture=$!
    sleep 1
    set +e
    "${adb[@]}" shell am instrument -w -r -e class com.zane.zanebox.SelectionCacheTest com.zane.locus.test/androidx.test.runner.AndroidJUnitRunner >"$out/selection-instrumentation.txt" 2>&1
    rc2=$?
    set -e
    if [[ "$rc2" -ne 0 ]]; then rc="$rc2"; fi
fi
"${adb[@]}" logcat -d >"$out/logcat.txt"
"${adb[@]}" shell find /sdcard/Android/data/com.zane.locus/files/ -type f >"$out/device-files-after.txt" 2>/dev/null || true
python3 - "$out" <<'CREATED'
import json,pathlib,sys
p=pathlib.Path(sys.argv[1])
before=set((p/'device-files-before.txt').read_text().splitlines()) if (p/'device-files-before.txt').exists() else set()
after=set((p/'device-files-after.txt').read_text().splitlines()) if (p/'device-files-after.txt').exists() else set()
(p/'created-device-files.json').write_text(json.dumps(sorted(after-before)))
CREATED
"${adb[@]}" pull /sdcard/Android/data/com.zane.locus/files/ "$out/device-files" >"$out/pull.txt" 2>&1 || true
python3 -B "$root/tests/verify_device_evidence.py" "$out"
python3 - "$out" "$rc" "$full_run" <<'PY'
import pathlib,sys,json,re
p=pathlib.Path(sys.argv[1]);rc=int(sys.argv[2]);full=sys.argv[3]=='1'
s=(p/'instrumentation.txt').read_text();print(s[-5000:])
reasons=[]
m1=re.search(r'OK \((\d+) tests?\)',s)
if rc!=0 or not m1 or 'FAILURES!!!' in s:reasons.append('instrumentation failed')
total=int(m1[1]) if m1 else 0
if full and not reasons:
    extra=(p/'selection-instrumentation.txt').read_text() if (p/'selection-instrumentation.txt').exists() else ''
    print(extra[-1200:])
    m2=re.search(r'OK \((\d+) tests?\)',extra)
    if not m2 or 'FAILURES!!!' in extra:reasons.append('selection instrumentation failed')
    else:total+=int(m2[1])
crash=(p/'logcat.txt').read_text()
if re.search(r'FATAL EXCEPTION[^\n]*\n(?:[^\n]*\n){0,6}[^\n]*com\.zane\.(locus|zanebox|probe)',crash):reasons.append('app crash')
status='FAIL' if reasons else 'PASS'
(p/'result.json').write_text(json.dumps({'status':status,'reasons':reasons,'instrumentationTests':total},indent=2))
if reasons:sys.exit(1)
PY
if [[ "$full_run" -eq 1 ]]; then
    if ! python3 -B "$root/tests/run_maestro_ui.py" optimization version --output "$out/maestro.json"; then
        python3 -c 'import json,pathlib,sys;p=pathlib.Path(sys.argv[1]);d=json.loads((p/"result.json").read_text());d["status"]="FAIL";d["reasons"]=d.get("reasons",[])+["maestro failed"];(p/"result.json").write_text(json.dumps(d,indent=2))' "$out"
        exit 1
    fi
fi
echo OPTIMIZATION_DEVICE_PASS
