#requires -Version 7
<#
  Mailhere'nin Microsoft Store (MSIX) paketini üretir (Windows, PowerShell 7).

    pwsh scripts/msix-paketle.ps1 -Exe target\release\mailhere.exe -Surum 0.11.0

  Kimlik değerleri Partner Center › Ürün kimliği sayfasından gelir; parametreyle
  ya da ortam değişkeniyle verilir (STORE_KIMLIK, STORE_YAYINCI,
  STORE_YAYINCI_ADI, STORE_AD). Üçü de boşsa DENEME kimliğiyle paketlenir:
  o paket yalnız yerel denemeye yarar, Store kabul etmez (dosya adı
  "-deneme" ile biter).

  Paket imzalanmaz: Store gönderimini Microsoft imzalar. Yerelde kurmak için
  scripts/msix-dene.ps1 kendinden imzalı bir sertifikayla imzalar.

  Gereken: Windows SDK (makeappx.exe, makepri.exe).
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)] [string] $Exe,
  [Parameter(Mandatory)] [string] $Surum,
  [string] $Kimlik = $env:STORE_KIMLIK,
  [string] $Yayinci = $env:STORE_YAYINCI,
  [string] $YayinciAdi = $env:STORE_YAYINCI_ADI,
  [string] $Ad = $env:STORE_AD,
  [string] $Cikti = 'target\msix'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$kok = Split-Path -Parent $PSScriptRoot
$sablon = Join-Path $kok 'src-tauri\msix\AppxManifest.xml'
$varliklar = Join-Path $kok 'src-tauri\msix\Assets'

if (-not (Test-Path $Exe)) { throw "Uygulama dosyası yok: $Exe. Önce 'pnpm tauri build --no-bundle' çalıştır." }
if ($Surum -notmatch '^\d+\.\d+\.\d+$') { throw "Sürüm X.Y.Z biçiminde olmalı (ön sürüm eki Store'da yok): $Surum" }
$paketSurumu = "$Surum.0"

# Kimlik: üçü birlikte verilir ya da hiçbiri (deneme).
$verilen = @($Kimlik, $Yayinci, $YayinciAdi | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }).Count
if ($verilen -eq 0) {
  $deneme = $true
  $Kimlik = 'Mailhere.Deneme'
  $Yayinci = 'CN=Mailhere Deneme'
  $YayinciAdi = 'Mailhere Deneme'
  Write-Host 'Kimlik değerleri verilmedi: DENEME kimliğiyle paketleniyor (Store kabul etmez).'
} elseif ($verilen -ne 3) {
  throw 'Kimlik eksik: STORE_KIMLIK, STORE_YAYINCI ve STORE_YAYINCI_ADI birlikte verilmeli (Partner Center › Ürün kimliği).'
} else {
  $deneme = $false
  if ($Yayinci -notmatch '^CN=') { throw "STORE_YAYINCI 'CN=' ile başlamalı (Package/Identity/Publisher değeri): $Yayinci" }
}
if ([string]::IsNullOrWhiteSpace($Ad)) { $Ad = 'Mailhere' }

# Windows SDK araçları: en yeni sürümün x64 klasörü.
function Arac([string] $ad) {
  $yol = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\$ad" -ErrorAction SilentlyContinue |
    Sort-Object { [version] $_.Directory.Parent.Name } | Select-Object -Last 1
  if (-not $yol) { throw "$ad bulunamadı. Windows SDK'yı kur (Visual Studio Installer › Windows 11 SDK)." }
  $yol.FullName
}
$makeappx = Arac 'makeappx.exe'
$makepri = Arac 'makepri.exe'

# Düzen klasörü: paketin içine girecek her şey.
New-Item -ItemType Directory -Force -Path $Cikti | Out-Null
$Cikti = (Resolve-Path $Cikti).Path
$duzen = Join-Path $Cikti 'duzen'
if (Test-Path $duzen) { Remove-Item -Recurse -Force -LiteralPath $duzen }
New-Item -ItemType Directory -Force -Path (Join-Path $duzen 'Assets') | Out-Null
Copy-Item -LiteralPath $Exe -Destination (Join-Path $duzen 'mailhere.exe')
Copy-Item -Path (Join-Path $varliklar '*.png') -Destination (Join-Path $duzen 'Assets')

# Bildirim: değerler XML'e kaçışlanarak yazılır.
function Xml([string] $s) { [System.Security.SecurityElement]::Escape($s) }
$bildirim = Get-Content -Raw -Encoding utf8 -LiteralPath $sablon
$bildirim = $bildirim.Replace('{{KIMLIK}}', (Xml $Kimlik)).Replace('{{YAYINCI}}', (Xml $Yayinci)).Replace('{{YAYINCI_ADI}}', (Xml $YayinciAdi)).Replace('{{AD}}', (Xml $Ad)).Replace('{{SURUM}}', $paketSurumu)
if ($bildirim -match '\{\{[A-Z_]+\}\}') { throw "Bildirimde doldurulmamış yer kaldı: $($Matches[0])" }
Set-Content -LiteralPath (Join-Path $duzen 'AppxManifest.xml') -Value $bildirim -Encoding utf8NoBOM

# Kaynak dizini: ölçek ve yüzey niteleyicili logolar buradan seçilir.
$priAyar = Join-Path $Cikti 'priconfig.xml'
& $makepri createconfig /cf $priAyar /dq en-US /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makepri createconfig başarısız ($LASTEXITCODE)" }
# Varsayılan ayar kaynakları dil ve ölçeğe göre ayrı .pri dosyalarına böler
# (paket demeti içindir). Tek pakette hepsi resources.pri'de durmalı.
$ayar = [xml] (Get-Content -Raw -LiteralPath $priAyar)
foreach ($bolme in @($ayar.SelectNodes('//packaging'))) { [void] $bolme.ParentNode.RemoveChild($bolme) }
$ayar.Save((Resolve-Path $priAyar).Path)
& $makepri new /pr $duzen /cf $priAyar /mn (Join-Path $duzen 'AppxManifest.xml') /of (Join-Path $duzen 'resources.pri') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makepri new başarısız ($LASTEXITCODE)" }

$ek = if ($deneme) { '-deneme' } else { '' }
$paket = Join-Path $Cikti "Mailhere_${Surum}_x64$ek.msix"
& $makeappx pack /d $duzen /p $paket /o
if ($LASTEXITCODE -ne 0) { throw "makeappx pack başarısız ($LASTEXITCODE)" }

if (-not (Test-Path $paket)) { throw "Paket oluşmadı: $paket" }
$boyut = [math]::Round((Get-Item $paket).Length / 1MB, 1)
Write-Host "Paket hazır: $paket ($boyut MB) · kimlik $Kimlik · sürüm $paketSurumu"
if ($env:GITHUB_OUTPUT) {
  "paket=$paket" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
  "kimlik=$Kimlik" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
  "yayinci=$Yayinci" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
  "deneme=$($deneme.ToString().ToLower())" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
}
