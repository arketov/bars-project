param([ValidateSet('vulkan','cuda','onnx','onnx124')][string]$Kind)
$ErrorActionPreference = 'Stop'
$barsFixture = Join-Path $PSScriptRoot 'fixture-prebuilt'
New-Item -ItemType Directory -Force $barsFixture | Out-Null
$barsPackages = @{
    vulkan = @{repo='ggml-org/llama.cpp';tag='b11953';name='llama-b11953-bin-win-vulkan-x64.zip';sha='31bc36c55e7f10939d3129fd671c4da8b4dacb0276892aaf573c7d4ab5d512eb'}
    cuda = @{repo='ggml-org/llama.cpp';tag='b11953';name='llama-b11953-bin-win-cuda-12.4-x64.zip';sha='45d9b8169c0ca8592df639f9462e4091f466fe63238bd748be16c636fbc3406d'}
    onnx = @{repo='microsoft/onnxruntime';tag='v1.28.0';name='onnxruntime-win-x64-gpu_cuda12-1.28.0.zip';sha='6b7bf16d6d30180db7f386fb179aa4e4f1313f0924531a2879b7b090b56518c1'}
    onnx124 = @{repo='microsoft/onnxruntime';tag='v1.24.4';name='onnxruntime-win-x64-gpu-1.24.4.zip';sha='ef3337a0b8184eb8beec310f7c83bd50376b3eefc43aab84ac8e452f6987df0a'}
}
$barsPackage=$barsPackages[$Kind]
$barsArchive=Join-Path $barsFixture $barsPackage.name
$barsUrl="https://github.com/$($barsPackage.repo)/releases/download/$($barsPackage.tag)/$($barsPackage.name)"
& curl.exe --fail --location --retry 3 --continue-at - --silent --show-error --output $barsArchive --url $barsUrl
if ($LASTEXITCODE -ne 0) { throw 'Archive download failed' }
$barsHash=(Get-FileHash -Algorithm SHA256 -LiteralPath $barsArchive).Hash.ToLowerInvariant()
if ($barsHash -ne $barsPackage.sha) { throw 'Archive digest mismatch' }
Expand-Archive -LiteralPath $barsArchive -DestinationPath (Join-Path $barsFixture $Kind) -Force
@{kind=$Kind;url=$barsUrl;sha256=$barsHash;bytes=(Get-Item -LiteralPath $barsArchive).Length} | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $barsFixture "$Kind.artifact.json")
Write-Output "Verified $Kind"
