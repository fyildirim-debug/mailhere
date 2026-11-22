#requires -Version 7
<#
  MSIX paketini bu bilgisayara kurar, açar ve çalıştığını doğrular (duman testi).

    pwsh scripts/msix-dene.ps1 -Paket target\msix\Mailhere_0.11.0_x64-deneme.msix `
      -Kimlik Mailhere.Deneme -Yayinci 'CN=Mailhere Deneme'

  Store paketi imzasızdır; kurulabilmesi için kopyası, yayıncı adıyla aynı
  konuya sahip kendinden imzalı geçici bir sertifikayla imzalanır ve sertifika
  LocalMachine\TrustedPeople deposuna eklenir (yönetici ister). Test bitince
  uygulama kaldırılır ve sertifika silinir (-Kalsin verilmediyse).

  Doğrulananlar: paket kuruluyor, uygulama açılıyor ve ayakta kalıyor, günlük
  dosyasında "Microsoft Store paketi" ve "veri klasörü" (çekirdek açıldı)
  satırları var, açılışta başlama görevi açık.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)] [string] $Paket,
  [Parameter(Mandatory)] [string] $Kimlik,
  [Parameter(Mandatory)] [string] $Yayinci,
  [string] $Cikti = 'target\msix\deneme',
  [int] $Bekle = 120,
  [switch] $Kalsin
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
New-Item -ItemType Directory -Force -Path $Cikti | Out-Null
# .NET çağrıları (dosya yazma, resim kaydetme) göreli yolu PowerShell'in
# konumuna göre çözmez: tam yol kullanılır.
$Cikti = (Resolve-Path $Cikti).Path
$Paket = (Resolve-Path $Paket).Path

function Arac([string] $ad) {
  $yol = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\$ad" -ErrorAction SilentlyContinue |
    Sort-Object { [version] $_.Directory.Parent.Name } | Select-Object -Last 1
  if (-not $yol) { throw "$ad bulunamadı. Windows SDK'yı kur." }
  $yol.FullName
}
# Appx komutları Windows PowerShell'de güvenilir çalışır.
function Appx([string] $komut) {
  $cikti = & powershell.exe -NoProfile -NonInteractive -Command $komut
  if ($LASTEXITCODE -ne 0) { throw "Appx komutu başarısız ($LASTEXITCODE): $komut" }
  $cikti
}
function EkranGoruntusu([string] $yol) {
  try {
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $alan = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $resim = [System.Drawing.Bitmap]::new($alan.Width, $alan.Height)
    $cizim = [System.Drawing.Graphics]::FromImage($resim)
    $cizim.CopyFromScreen($alan.Location, [System.Drawing.Point]::Empty, $alan.Size)
    $resim.Save($yol, [System.Drawing.Imaging.ImageFormat]::Png)
    $cizim.Dispose(); $resim.Dispose()
    Write-Host "Ekran görüntüsü: $yol"
  } catch {
    Write-Host "Ekran görüntüsü alınamadı: $($_.Exception.Message)"
  }
}

# 1) Geçici sertifika (konu = yayıncı), paketin kopyasını imzala.
$rsa = [System.Security.Cryptography.RSA]::Create(2048)
$istek = [System.Security.Cryptography.X509Certificates.CertificateRequest]::new(
  $Yayinci, $rsa, [System.Security.Cryptography.HashAlgorithmName]::SHA256,
  [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
$istek.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509BasicConstraintsExtension]::new($false, $false, 0, $true))
$istek.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509KeyUsageExtension]::new(
    [System.Security.Cryptography.X509Certificates.X509KeyUsageFlags]::DigitalSignature, $true))
$amac = [System.Security.Cryptography.OidCollection]::new()
[void] $amac.Add([System.Security.Cryptography.Oid]::new('1.3.6.1.5.5.7.3.3'))
$istek.CertificateExtensions.Add([System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($amac, $true))
$sertifika = $istek.CreateSelfSigned([DateTimeOffset]::UtcNow.AddDays(-1), [DateTimeOffset]::UtcNow.AddDays(3))
$parola = [guid]::NewGuid().ToString('N')
$pfx = Join-Path $Cikti 'deneme.pfx'
[IO.File]::WriteAllBytes($pfx, $sertifika.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Pfx, $parola))

$imzali = Join-Path $Cikti 'Mailhere-imzali-deneme.msix'
Copy-Item -LiteralPath $Paket -Destination $imzali -Force
& (Arac 'signtool.exe') sign /fd SHA256 /f $pfx /p $parola $imzali
if ($LASTEXITCODE -ne 0) { throw "signtool başarısız ($LASTEXITCODE). Yayıncı adı bildirimdekiyle aynı mı: $Yayinci" }
Remove-Item -LiteralPath $pfx -Force

$acik = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
  $sertifika.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))
$depo = [System.Security.Cryptography.X509Certificates.X509Store]::new('TrustedPeople', 'LocalMachine')
$depo.Open('ReadWrite')
$depo.Add($acik)
$depo.Close()

$hata = $null
$aile = $null
try {
  # 2) Kur.
  Appx "Add-AppxPackage -Path '$imzali'" | Out-Null
  $aile = (Appx "(Get-AppxPackage -Name '$Kimlik').PackageFamilyName" | Select-Object -First 1)
  if ([string]::IsNullOrWhiteSpace($aile)) { throw "Paket kuruldu ama bulunamadı: $Kimlik" }
  Write-Host "Kuruldu: $aile"

  # 3) Aç (kabuk etkinleştirmesi; olmazsa Gezgin üzerinden).
  $hedef = "shell:AppsFolder\$aile!Mailhere"
  Start-Process $hedef
  Start-Sleep -Seconds 15
  if (-not (Get-Process -Name mailhere -ErrorAction SilentlyContinue)) {
    Write-Host 'İlk açılış denemesinde süreç yok; Gezgin üzerinden deneniyor.'
    Start-Process explorer.exe $hedef
  }

  # 4) Günlükte beklenen satırlar çıkana kadar bekle.
  $adaylar = @(
    (Join-Path $env:LOCALAPPDATA "Packages\$aile\LocalCache\Local\com.furkanyildirim.mailhere\logs\mailhere.log"),
    (Join-Path $env:LOCALAPPDATA 'com.furkanyildirim.mailhere\logs\mailhere.log')
  )
  $beklenen = @('Microsoft Store paketi', 'veri klasörü', 'açılışta başlama')
  $son = (Get-Date).AddSeconds($Bekle)
  $gunluk = $null
  $metin = ''
  while ((Get-Date) -lt $son) {
    Start-Sleep -Seconds 3
    $gunluk = $adaylar | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($gunluk) {
      $metin = Get-Content -Raw -Encoding utf8 -LiteralPath $gunluk
      if (-not ($beklenen | Where-Object { $metin -notlike "*$_*" })) { break }
    }
  }
  Start-Sleep -Seconds 5
  EkranGoruntusu (Join-Path $Cikti 'ekran.png')
  if ($gunluk) {
    Copy-Item -LiteralPath $gunluk -Destination (Join-Path $Cikti 'mailhere.log') -Force
    Write-Host "Günlük: $gunluk"
    Write-Host '--- günlük ---'
    Write-Host $metin
    Write-Host '--- son ---'
  }

  $surec = Get-Process -Name mailhere -ErrorAction SilentlyContinue
  if (-not $surec) { throw 'Uygulama açılmadı ya da hemen kapandı (mailhere süreci yok).' }
  if (-not $gunluk) { throw "Günlük dosyası oluşmadı; uygulama çekirdeği başlatamamış olabilir. Bakılan yerler: $($adaylar -join '; ')" }
  $eksik = @($beklenen | Where-Object { $metin -notlike "*$_*" })
  if ($eksik.Count) { throw "Günlükte beklenen satır yok: $($eksik -join ', ')" }
  if ($metin -notmatch 'açılışta başlama.*acik=true|acik=true.*açılışta başlama') {
    throw 'Açılışta başlama görevi açılmamış (günlükte acik=true yok).'
  }
  Write-Host 'Duman testi geçti: paket kuruldu, uygulama açıldı, Mağaza kipi ve açılışta başlama doğrulandı.'
} catch {
  $hata = $_
  EkranGoruntusu (Join-Path $Cikti 'ekran-hata.png')
} finally {
  if (-not $Kalsin) {
    Get-Process -Name mailhere -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if ($aile) {
      try { Appx "Get-AppxPackage -Name '$Kimlik' | Remove-AppxPackage" | Out-Null } catch { Write-Host "Kaldırılamadı: $($_.Exception.Message)" }
    }
    $depo = [System.Security.Cryptography.X509Certificates.X509Store]::new('TrustedPeople', 'LocalMachine')
    $depo.Open('ReadWrite')
    $depo.Remove($acik)
    $depo.Close()
  }
}
if ($hata) { throw $hata }
