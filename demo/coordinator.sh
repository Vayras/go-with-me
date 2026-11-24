#!/bin/bash
# go-with-me demo: a scripted coordinator (the real hub runs Claude Code here)
B=$'\033[1m'; D=$'\033[2m'; G=$'\033[32m'; Y=$'\033[33m'; C=$'\033[36m'; O=$'\033[38;5;209m'; R=$'\033[0m'
type_out() { local s="$1" i; for ((i=0;i<${#s};i++)); do printf '%s' "${s:$i:1}"; sleep 0.03; done; echo; }
clear
printf '%s✻ Coordinator%s %s— sees every machine and session%s\n\n' "$O$B" "$R" "$D" "$R"
sleep 2
printf '%s❯%s ' "$C" "$R"; type_out "summarise everything"
sleep 0.8
printf '  %s⏺ gwm status · gwm peek landing-page · gwm peek api-tests · gwm peek load-test%s\n\n' "$D" "$R"; sleep 1.2
printf '  %slanding-page%s (studio)   building the hero section — build passing, 2 files changed\n' "$B" "$R"; sleep 0.5
printf '  %sapi-tests%s (office-1)    fixed the retry bug — %s148/148 tests pass%s, waiting for review\n' "$B" "$R" "$G" "$R"; sleep 0.5
printf '  %sload-test%s (sandbox VM)  done — p95 182ms, report written\n' "$B" "$R"; sleep 0.5
printf '  %slocal-refactor%s (studio) idle, on the local model — no cloud tokens used\n' "$B" "$R"; sleep 0.5
printf '  %s⚠ macbook is connected to studio (1 tab)%s\n\n' "$Y" "$R"; sleep 2.5
printf '%s❯%s ' "$C" "$R"; type_out "tell load-test to rerun with 500 users"
sleep 0.6
printf '  %s⏺ gwm send load-test "rerun the load test with --vus 500"%s\n' "$D" "$R"; sleep 0.8
printf '  %s✓ sent to load-test%s\n' "$G" "$R"
while true; do printf '\n%s❯%s ' "$C" "$R"; read -r line || exit 0; [ -n "$line" ] && printf '  %s(demo) The real coordinator is Claude Code and would act on: %s%s\n' "$D" "$line" "$R"; done
