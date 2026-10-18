param(
    [Parameter(Mandatory)][string]$Destination,
    [string]$Archive
)
$ErrorActionPreference = 'Stop'
$version = '1.4.363.0'
$expected = 'a25a927aa8b9f0371048f1861cf88ac3b9bc9b1fb332c42d897c8ab32695769a'
$cache = Join-Path $env:LOCALAPPDATA "bars/build-deps/vulkan-$version"
New-Item -ItemType Directory -Force $cache | Out-Null
if (-not $Archive) {
    $Archive = Join-Path $cache 'components.zip'
    if (-not (Test-Path -LiteralPath $Archive)) {
        Invoke-WebRequest "https://sdk.lunarg.com/sdk/download/$version/windows/VulkanRT-X64-$version-Components.zip" -OutFile $Archive
    }
}
if ((Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Vulkan runtime: SHA-256 архива не совпал; упаковка остановлена'
}
Expand-Archive -LiteralPath $Archive -DestinationPath $cache -Force
$source = Join-Path $cache "VulkanRT-X64-$version-Components"
$dll = Join-Path $source 'x64/vulkan-1.dll'
$signature = Get-AuthenticodeSignature -FilePath $dll
if ($signature.Status -ne 'Valid') { throw 'Vulkan runtime: подпись DLL недействительна' }
New-Item -ItemType Directory -Force $Destination | Out-Null
Copy-Item -LiteralPath $dll -Destination (Join-Path $Destination 'vulkan-1.dll') -Force
Copy-Item -LiteralPath (Join-Path $source 'VulkanRT-License.txt') -Destination (Join-Path $Destination 'VulkanRT-License.txt') -Force
Write-Output "Vulkan loader $version подготовлен для упаковки; системный runtime не изменён"
