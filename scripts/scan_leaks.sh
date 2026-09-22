#!/usr/bin/env bash
# Leak / licensed-string scanner — a required pre-push gate, co-equal to gitleaks.
# gitleaks catches key-shaped secrets; this catches the NON-key-shaped leaks it misses:
# host absolute paths, licensed-data identifiers, the copyrighted book, the local dev DSN.
# Exits non-zero (fails CI) if any forbidden string appears anywhere in the tree.
set -uo pipefail
ROOT="${1:-.}"

# Unambiguous leaks only — host abs-paths, licensed-data identifiers, the dev DSN, the copyrighted book, real token prefixes.
# (Deliberately NOT flagging public API hostnames like api-fxtrade/api-fxpractice.oanda.com or env-var NAMES like
#  OANDA_API_TOKEN — those are public/legitimate; actual secret VALUES are gitleaks' job.)
PATTERNS='SHARADAR_|[Ss]haradar|Nasdaq Data Link|Astra_Forex|ASTRA_Working|load_sharadar|astra_dev_2025|Advances in Financial Machine Learning|github_pat_'

# scan_leaks.sh and the CI workflow legitimately NAME the forbidden tokens (to keep them out /
# to assert a licensed loader's absence), so they are exempt from this content grep.
# gitleaks still scans them for real secrets.
HITS=$(grep -rInE "$PATTERNS" "$ROOT" \
  --exclude-dir=.git --exclude-dir=target --exclude-dir=__pycache__ \
  --exclude-dir=.pytest_cache --exclude-dir=.mypy_cache --exclude-dir=.ruff_cache \
  --exclude-dir=node_modules --exclude-dir=.venv \
  --exclude="scan_leaks.sh" --exclude="ci.yml" 2>/dev/null)

if [ -n "$HITS" ]; then
  echo "❌ LEAK SCAN FAILED — forbidden host-path / licensed / credential strings found:"
  echo "$HITS"
  exit 1
fi
echo "✅ LEAK SCAN CLEAN — no host-paths, licensed identifiers, or credential patterns."
exit 0
