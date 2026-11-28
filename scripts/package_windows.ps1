# Split release Windows: satu zip per EP (cpu/cuda/directml/vulkan).
# Jalankan dari root repo:
#   pwsh -File scripts/package_windows.ps1
#   pwsh -File scripts/package_windows.ps1 -Variants cpu,cuda -CudaRuntimeDir C:\cuda\bin
param(
  [string]$OutDir = "out/release",
  [ValidateSet("cpu", "cuda", "directml", "vulkan")]
  [string[]]$Variants = @("cpu", "cuda", "directml", "vulkan"),
  [string]$TargetDir = "target/package",
  [string]$CudaRuntimeDir = "",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$ffmpegBin = "E:\temp\opencode\ffmpeg-9.0.2-full_build-shared\bin"
$env:PATH = "$ffmpegBin;$env:PATH"

$featureMap = @{
  cpu      = @()
  cuda     = @("gpu-cuda")
  directml = @("gpu-directml")
  vulkan   = @("gpu-webgpu", "wgpu-lyon")
}
# CLI tidak punya feature gpu-cuda/gpu-directml/gpu-webgpu; pakai feature
# fframes yang setara agar build CLI tidak gagal karena feature tak dikenal.
$cliFeatureMap = @{
  cpu      = @()
  cuda     = @("gpu-nvidia")
  directml = @()
  vulkan   = @("gpu-vulkan", "wgpu-lyon")
}

function Build-Variant([string]$variant) {
  $features = $featureMap[$variant]
  $cliFeatures = $cliFeatureMap[$variant]
  $featArgs = @()
  if ($cliFeatures.Count -gt 0) { $featArgs = @("--features", ($cliFeatures -join ",")) }

  Write-Host "[package] build CLI $variant"
  & cargo build --release --target-dir $TargetDir -p rscaps-cli --bin rscaps @featArgs
  if ($LASTEXITCODE -ne 0) { throw "build CLI $variant gagal" }
  Copy-Item (Join-Path $TargetDir "release\rscaps.exe") (Join-Path $TargetDir "release\rscaps-cli.exe") -Force

  Write-Host "[package] build Tauri $variant"
  $tauriFeatures = @("custom-protocol") + $features
  & cargo build --release --target-dir $TargetDir -p rscaps --no-default-features --features ($tauriFeatures -join ",")
  if ($LASTEXITCODE -ne 0) { throw "build Tauri $variant gagal" }
}

function Stage-Variant([string]$variant) {
  $stage = Join-Path $OutDir "rscaps-$variant"
  Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $stage | Out-Null

  Copy-Item (Join-Path $TargetDir "release\rscaps-cli.exe") (Join-Path $stage "rscaps-cli.exe")
  Copy-Item (Join-Path $TargetDir "release\rscaps.exe") (Join-Path $stage "rscaps-studio.exe")

  # Runtime DLL yang dibutuhkan varian ini saja. target/release bisa memuat
  # sisa build varian lain (mis. DirectML.dll saat build CPU) — jangan ikut
  # dibawa ke zip yang salah.
  $commonDlls = @("app_lib.dll", "onnxruntime.dll", "onnxruntime_providers_shared.dll")
  $variantDlls = switch ($variant) {
    "cuda"     { @("onnxruntime_providers_cuda.dll", "cudart64_*.dll", "cublas64_*.dll", "cublasLt64_*.dll", "cudnn64_*.dll") }
    "directml" { @("DirectML.dll", "onnxruntime_providers_dml.dll") }
    "vulkan"   { @("webgpu_dawn.dll", "dxcompiler.dll", "dxil.dll", "vulkan-1.dll") }
    default    { @() }
  }
  $releaseDir = Join-Path $TargetDir "release"
  $ffmpegDlls = Get-ChildItem $ffmpegBin -Filter *.dll -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^(av|sw)[a-z0-9-]+\.dll$' } |
    Select-Object -ExpandProperty Name
  foreach ($pattern in @($commonDlls + $variantDlls + $ffmpegDlls)) {
    Get-ChildItem $releaseDir -Filter $pattern -ErrorAction SilentlyContinue |
      Copy-Item -Destination $stage -Force
    Get-ChildItem $ffmpegBin -Filter $pattern -ErrorAction SilentlyContinue |
      Copy-Item -Destination $stage -Force
  }
  foreach ($exe in @("ffmpeg.exe", "ffprobe.exe")) {
    $p = Join-Path $ffmpegBin $exe
    if (Test-Path $p) { Copy-Item $p $stage }
  }

  if ($variant -eq "cuda" -and $CudaRuntimeDir) {
    Get-ChildItem $CudaRuntimeDir -Filter *.dll -ErrorAction SilentlyContinue |
      Copy-Item -Destination $stage
  }
  if ($variant -eq "vulkan") {
    foreach ($dll in @("webgpu_dawn.dll", "dxcompiler.dll", "dxil.dll")) {
      $p = Join-Path $ffmpegBin $dll
      if (Test-Path $p) { Copy-Item $p $stage }
    }
  }

  $zip = Join-Path $OutDir "rscaps-$variant.zip"
  Remove-Item -Force $zip -ErrorAction SilentlyContinue
  Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip
  Write-Host "[package] $zip"
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
foreach ($v in $Variants) {
  if (-not $SkipBuild) { Build-Variant $v }
  Stage-Variant $v
}
Write-Host "[package] selesai: $OutDir"
