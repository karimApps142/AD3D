#!/bin/bash
# Run the bundled app in spike mode N times; each run writes <OUTROOT>/run<k>/report.json.
# Usage: [START=k] [PROBE=1] [MAXIMIZE=1] [MAX_LOAD=8] [FORCE=1] [OUTROOT=dir] docs/spike/run_spike.sh [N]
#
# C4/C5 depend on machine contention (audit round 1: at 1-minute load 13-157, or with
# Docker/VM processes busy, C4 went to 220-240 ms and C5 to 14-26 updates/s). So:
# - the 1-minute load average and the top CPU processes are logged before and after
#   every run, and merged into report.json as "host" (with "quiet": load <= MAX_LOAD);
# - a run does not start while the 1-minute load is above MAX_LOAD (default 8)
#   unless FORCE=1, in which case the report is flagged "quiet": false.
# PROBE=1 runs system_profiler at +3 s to see which GPU drives the display; it
# overlaps C4/C5 and caused a 1.1 s GPU stall once, so primary runs leave it off.
# MAXIMIZE=1 maximises the window (AD3D_SPIKE_MAXIMIZE=1) for a full-screen canvas run.
# Only the PID this script started is ever killed.
set -u
ROOT=/Users/apple/Desktop/workspace/nextjs/AD3D
BIN=$ROOT/target/universal-apple-darwin/release/bundle/macos/AD3D.app/Contents/MacOS/ad3d-app
OUTROOT=${OUTROOT:-$ROOT/docs/spike}
MAX_LOAD=${MAX_LOAD:-8}
N=${1:-3}
load1() { sysctl -n vm.loadavg | awk '{print $2}'; }
top_cpu() { ps -Ao %cpu,comm -r | head -8; }
for k in $(seq "${START:-1}" "$N"); do
  OUT=$OUTROOT/run$k
  L=$(load1)
  if awk -v l="$L" -v m="$MAX_LOAD" 'BEGIN{exit !(l > m)}'; then
    if [ "${FORCE:-0}" != 1 ]; then
      echo "run$k NOT started: 1-minute load $L > MAX_LOAD $MAX_LOAD (stop Docker/VMs/builds, or FORCE=1 to run flagged)"
      top_cpu
      continue
    fi
  fi
  rm -rf "$OUT"; mkdir -p "$OUT"
  echo "run$k start $(date +%T) load: $(sysctl -n vm.loadavg)"
  top_cpu > "$OUT/top-cpu-before.txt"
  LOAD_BEFORE=$(sysctl -n vm.loadavg)
  AD3D_SPIKE=1 AD3D_SPIKE_OUT=$OUT AD3D_SPIKE_MAXIMIZE=${MAXIMIZE:-0} "$BIN" >"$OUT/app-stdout.txt" 2>"$OUT/app-stderr.txt" &
  PID=$!
  T0=$(date +%s)
  GPU=""
  while kill -0 $PID 2>/dev/null; do
    if [ "${PROBE:-0}" = 1 ] && [ -z "$GPU" ] && [ $(( $(date +%s) - T0 )) -ge 3 ]; then
      # Which GPU drives the built-in display while the app is up (WebKit masks the renderer).
      GPU=$(system_profiler SPDisplaysDataType 2>/dev/null | awk '/Chipset Model/{c=$0} /Color LCD/{print c}')
      echo "$GPU" > "$OUT/display-gpu-during-run.txt"
    fi
    if [ $(( $(date +%s) - T0 )) -ge 120 ]; then echo "run$k timeout, killing $PID"; kill $PID; break; fi
    sleep 0.5
  done
  wait $PID; RC=$?
  LOAD_AFTER=$(sysctl -n vm.loadavg)
  top_cpu > "$OUT/top-cpu-after.txt"
  if [ -f "$OUT/report.json" ]; then
    LOAD_BEFORE="$LOAD_BEFORE" LOAD_AFTER="$LOAD_AFTER" MAX_LOAD="$MAX_LOAD" OUT="$OUT" MAXIMIZE="${MAXIMIZE:-0}" node -e '
      const fs = require("fs");
      const p = process.env.OUT + "/report.json";
      const r = JSON.parse(fs.readFileSync(p, "utf8"));
      const nums = (s) => s.replace(/[{}]/g, "").trim().split(/\s+/).map(Number);
      const before = nums(process.env.LOAD_BEFORE), after = nums(process.env.LOAD_AFTER);
      const read = (f) => { try { return fs.readFileSync(process.env.OUT + "/" + f, "utf8").trim().split("\n"); } catch { return null; } };
      r.host = {
        loadAvgBefore: before, loadAvgAfter: after, maxLoad: Number(process.env.MAX_LOAD),
        quiet: Math.max(before[0], after[0]) <= Number(process.env.MAX_LOAD),
        maximized: process.env.MAXIMIZE === "1",
        topCpuBefore: read("top-cpu-before.txt"), topCpuAfter: read("top-cpu-after.txt"),
      };
      fs.writeFileSync(p, JSON.stringify(r, null, 2));
    '
  fi
  echo "run$k end load: $LOAD_AFTER $(date +%T) exit=$RC elapsed=$(( $(date +%s) - T0 ))s report=$([ -f "$OUT/report.json" ] && echo yes || echo no) gpu=[$GPU]"
  sleep 3
done
