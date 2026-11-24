#!/bin/bash
# go-with-me demo: a pretend agent session (no AI, no network). Usage: agent.sh <session-name>
n="${1:-session}"
B=$'\033[1m'; D=$'\033[2m'; G=$'\033[32m'; Y=$'\033[33m'; C=$'\033[36m'; O=$'\033[38;5;209m'; R=$'\033[0m'
say() { printf '%s\n' "$1"; sleep "${2:-0.6}"; }
case "$n" in
  landing-page) eng="Claude Code · account: personal · studio"; title="Building hero section"
    steps=("Read src/pages/index.tsx" "Read src/components/Hero.tsx" "Edit src/components/Hero.tsx  (+42 −9)" "Bash npm run build" "✓ build passed in 4.1s" "Edit src/styles/hero.css  (+18 −2)");;
  api-tests) eng="Claude Code · account: work · office-1"; title="Fixing flaky API tests"
    steps=("Bash pytest tests/api -x -q" "✗ 2 failed: test_orders_retry, test_refund_timeout" "Read app/payments/retry.py" "Edit app/payments/retry.py  (+11 −4)" "Bash pytest tests/api -q" "✓ 148 passed in 31.8s");;
  local-refactor) eng="opencode · ollama/inkling-2-mini (local) · studio"; title="Refactor CLI arg parsing"
    steps=("read cli/args.go" "edit cli/args.go  (+64 −71)" "bash go test ./cli/..." "✓ ok cli 0.41s");;
  load-test) eng="Claude Code · account: personal · sandbox (Linux VM)"; title="Load test finished"
    steps=("Bash k6 run load.js --vus 200" "p95 latency 182ms · 0.02% errors" "✓ Wrote report to reports/load-2026-11-24.md");;
  *) exec "${SHELL:-/bin/zsh}" -l;;
esac
printf '\033]2;✳ %s\007' "$title"
clear
case "$eng" in Claude*) cmd=claude;; opencode*) cmd=opencode;; *) cmd="";; esac
if [ -n "$cmd" ]; then printf '%s%s $%s ' "$D" "${n}" "$R"; sleep 0.6; printf '%s\n\n' "$cmd"; sleep 0.6; fi
printf '%s╭──────────────────────────────────────────────────────────╮%s\n' "$O" "$R"
printf '%s│%s %s✻ %s%-52s %s│%s\n' "$O" "$R" "$O" "$B" "$title" "$O" "$R"
printf '%s│%s %s%-56s%s %s│%s\n' "$O" "$R" "$D" "$eng" "$R" "$O" "$R"
printf '%s╰──────────────────────────────────────────────────────────╯%s\n\n' "$O" "$R"
say "${C}❯${R} $(echo "$title" | tr "[:upper:]" "[:lower:]"), then run the checks" 1
for s in "${steps[@]}"; do
  case "$s" in ✓*) say "  ${G}${s}${R}" 0.9;; ✗*|p95*) say "  ${Y}${s}${R}" 0.9;; *) say "  ${D}⏺${R} ${s}" 0.8;; esac
done
say ""; say "${B}Done.${R} ${D}Waiting for your next message.${R}" 0
while true; do printf '\n%s❯%s ' "$C" "$R"; read -r line || exit 0; [ -n "$line" ] && say "  ${D}(demo) In a real session, the agent would work on: ${line}${R}" 0; done
