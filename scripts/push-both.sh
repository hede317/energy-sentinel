#!/usr/bin/env bash
# 双端推送 + 一致性自检：GitHub(origin, 直连) + Gitee(gitee, 直连)
# 用法: bash scripts/push-both.sh [提交信息]   —— 有未提交改动时必须传提交信息
set -euo pipefail
cd "$(dirname "$0")/.."

MSG="${1:-}"
if [ -n "$(git status --porcelain)" ]; then
  [ -z "$MSG" ] && { echo "❌ 有未提交改动，请附提交信息: bash scripts/push-both.sh \"feat: ...\""; exit 1; }
  git add -A
  git -c user.name="hede317" -c user.email="hede317@users.noreply.github.com" commit -m "$MSG"
fi

echo "── push GitHub (via Clash 7897)"
git -c http.proxy= push origin main
echo "── push Gitee (直连)"
git -c http.proxy= -c https.proxy= push gitee main

echo "── 一致性自检"
GH=$(git ls-remote origin main | cut -c1-10)
GE=$(git -c http.proxy= -c https.proxy= ls-remote gitee main | cut -c1-10)
git fetch origin main -q
git fetch gitee main -q
DIFF=$(git diff origin/main gitee/main --stat | tail -1)
echo "GitHub main → $GH"
echo "Gitee  main → $GE"
if [ "$GH" = "$GE" ]; then
  echo "✅ 双端一致 ($GH) ${DIFF:-树内容零差异}"
else
  echo "❌ 双端不一致！请检查报错"; exit 1
fi
