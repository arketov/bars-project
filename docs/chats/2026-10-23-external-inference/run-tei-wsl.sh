#!/usr/bin/env bash
# Изолированные серверы аппаратной проверки 5.3. Веса читаются только с D:.
set -euo pipefail
tei_run_root=/root/Projects/oss/bars-tei-5-3
case "${1:-}" in
  embed)
    tei_run_model=/mnt/d/text-generation-webui-1.16/models/tei-paraphrase-multilingual-MiniLM-L12-v2
    tei_run_port=18085
    ;;
  rerank)
    tei_run_model=/mnt/d/text-generation-webui-1.16/models/tei-mmarco-mMiniLMv2-L12-H384-v1
    tei_run_port=18086
    ;;
  *) echo 'expected embed or rerank' >&2; exit 2 ;;
esac
test -s "$tei_run_model/model.safetensors"
test -s "$tei_run_model/tokenizer.json"
printf '%s\n' "$$" > "$tei_run_root/$1.pid"
export LD_LIBRARY_PATH="/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export HF_HUB_OFFLINE=1
exec "$tei_run_root/runtime/usr/local/bin/text-embeddings-router" \
  --model-id "$tei_run_model" --hostname 0.0.0.0 --port "$tei_run_port" \
  --dtype float16 --max-batch-tokens 2048 --max-concurrent-requests 8 \
  --tokenization-workers 2 --json-output
