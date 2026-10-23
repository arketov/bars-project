#!/usr/bin/env bash
# Продолжение публичного OCI blob; итог принимается только по закреплённому SHA-256.
set -euo pipefail
tei_download_root=/root/Projects/oss/bars-tei-5-3
cd "$tei_download_root"
tei_download_token=$(curl --http1.1 -fLsS --connect-timeout 15 --max-time 30 \
  'https://ghcr.io/token?service=ghcr.io&scope=repository:huggingface/text-embeddings-inference:pull' \
  | sed -nE 's/.*"token"[[:space:]]*:[[:space:]]*"([^"]*)".*/\1/p')
test -n "$tei_download_token"
curl --http1.1 -fLsS --retry 0 --connect-timeout 15 --max-time 900 --continue-at - \
  --header "Authorization: Bearer $tei_download_token" \
  'https://ghcr.io/v2/huggingface/text-embeddings-inference/blobs/sha256:c48fda7c697bef18c4b7c7cb6de369fe94277b4119d1bc1f830e5a525240a952' \
  --output tei-binary-layer.tar.gz --write-out 'HTTP %{http_code}; received %{size_download}; seconds %{time_total}\n'
unset tei_download_token
echo 'c48fda7c697bef18c4b7c7cb6de369fe94277b4119d1bc1f830e5a525240a952  tei-binary-layer.tar.gz' | sha256sum -c
mkdir -p runtime
tar -xzf tei-binary-layer.tar.gz -C runtime
LD_LIBRARY_PATH=/usr/lib/wsl/lib ldd runtime/usr/local/bin/text-embeddings-router
LD_LIBRARY_PATH=/usr/lib/wsl/lib runtime/usr/local/bin/text-embeddings-router --version
