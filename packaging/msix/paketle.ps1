# Microsoft Store paketi (MSIX) üretir. Önce `pnpm tauri build --no-bundle`
# ile target/release/mailhere.exe derlenmiş olmalı. Çıktı: dist-msix/MailHere_<sürüm>_x64.msix
# Paket imzasızdır; Mağaza'ya yüklenince Microsoft imzalar.
param(
  [string]$Exe = "target/release/mailhere.exe",
  [string]$Cikti = "dist-msix"
)
$ErrorActionPreference = 'Stop'

$surum = (node -p "require('./package.json').version").Trim()
if ($surum -notmatch '^\d+\.\d+\.\d+$') { throw "Mağaza sürümü yalnız X.Y.Z olabilir (ön sürüm eki yok): $surum" }
$paketSurumu = "$surum.0"

if (-not (Test-Path $Exe)) { throw "Derlenmiş uygulama yok: $Exe" }

# Windows SDK'daki en yeni makeappx.
$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" |
  Sort-Object { [version]($_.Directory.Parent.Name) } -Descending |
  Select-Object -First 1
if (-not $makeappx) { throw "makeappx.exe bulunamadı (Windows SDK gerekli)." }

$kok = Join-Path $Cikti "icerik"
Remove-Item $Cikti -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $kok "Assets") | Out-Null

Copy-Item $Exe (Join-Path $kok "mailhere.exe")
Copy-Item "packaging/msix/Assets/*.png" (Join-Path $kok "Assets")
(Get-Content "packaging/msix/AppxManifest.xml" -Raw -Encoding utf8).Replace("__SURUM__", $paketSurumu) |
  Set-Content (Join-Path $kok "AppxManifest.xml") -Encoding utf8 -NoNewline

$msix = Join-Path $Cikti "MailHere_${paketSurumu}_x64.msix"
& $makeappx.FullName pack /o /d $kok /p $msix
if ($LASTEXITCODE -ne 0) { throw "makeappx başarısız ($LASTEXITCODE)" }
Write-Host "Paket: $msix"
