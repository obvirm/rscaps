# Contact sheet: baris = [tscaps | rscaps] per template (PS7).
param(
  [string]$Dir = "E:\temp\opencode\compare",
  [int]$PairsPerSheet = 6,
  [int]$CellH = 260
)
Add-Type -AssemblyName System.Drawing

$results = Get-Content (Join-Path $Dir "_results.json") | ConvertFrom-Json
$ok = @($results | Where-Object { $_.status -eq "ok" })
$font = New-Object System.Drawing.Font("Consolas", 13, [System.Drawing.FontStyle]::Bold)
$brush = [System.Drawing.Brushes]::White

$sheetIndex = 0
for ($s = 0; $s -lt [math]::Ceiling($ok.Count / $PairsPerSheet); $s++) {
  $start = $s * $PairsPerSheet
  $end = [math]::Min($start + $PairsPerSheet - 1, $ok.Count - 1)
  $batch = @($ok[$start..$end])

  $cells = New-Object System.Collections.Generic.List[object]
  $maxW = 0
  foreach ($r in $batch) {
    $imgA = [System.Drawing.Image]::FromFile((Join-Path $Dir "$($r.id)__tscaps.png"))
    $imgB = [System.Drawing.Image]::FromFile((Join-Path $Dir "$($r.id)__rscaps.png"))
    $wa = [int][math]::Round($imgA.Width * $CellH / $imgA.Height)
    $wb = [int][math]::Round($imgB.Width * $CellH / $imgB.Height)
    $cw = [int]($wa + $wb + 14)
    if ($cw -gt $maxW) { $maxW = $cw }
    $cells.Add([pscustomobject]@{ Id = $r.id; A = $imgA; B = $imgB; WA = $wa; WB = $wb })
  }

  $labelH = 26
  $sheetW = [int]($maxW + 16)
  $sheetH = [int]($cells.Count * ($CellH + $labelH + 10) + 8)
  $bmp = New-Object System.Drawing.Bitmap($sheetW, $sheetH)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::FromArgb(255, 10, 10, 12))
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic

  $y = 6
  $penA = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(90, 120, 230), 2)
  $penB = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(163, 230, 53), 2)
  foreach ($c in $cells) {
    $label = "$($c.Id)   [kiri=tscaps asli | kanan=rscaps]"
    $g.DrawString($label, $font, $brush, [float]6, [float]$y)
    $y += $labelH
    $rectA = [System.Drawing.Rectangle]::new(6, $y, [int]$c.WA, [int]$CellH)
    $rectB = [System.Drawing.Rectangle]::new(6 + [int]$c.WA + 8, $y, [int]$c.WB, [int]$CellH)
    $g.DrawImage($c.A, $rectA)
    $g.DrawImage($c.B, $rectB)
    $g.DrawRectangle($penA, $rectA)
    $g.DrawRectangle($penB, $rectB)
    $y += $CellH + 10
    $c.A.Dispose()
    $c.B.Dispose()
  }
  $g.Dispose()
  $sheetIndex++
  $out = Join-Path $Dir ("_sheet_{0:D2}.png" -f $sheetIndex)
  $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Write-Output "sheet: $out"
}
