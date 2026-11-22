# Mailhere — Microsoft Store (MSIX)

Mailhere'nin Microsoft Store sürümü, masaüstü sürümüyle **aynı ikilidir**
(`mailhere.exe`); yalnız MSIX paketi içinde çalıştığını anlayınca üç şeyi
farklı yapar. Paketi Actions üretir, paketi Microsoft imzalar; ücretli kod
imzası sertifikası gerekmez.

## Neden MSIX

Store iki tür Win32 uygulaması kabul eder:

| | EXE/MSI kurucusunu bağlamak | MSIX paketi |
|---|---|---|
| Kod imzası | Güvenilir kuruluştan ücretli sertifika zorunlu | Gerekmez; Store paketi kendisi imzalar |
| Güncelleme | Uygulamanın kendi güncelleyicisi | Store |
| SmartScreen uyarısı | Sertifikanın itibarına bağlı | Yok |

Kurucularımız (NSIS, MSI) Windows kod imzası taşımaz; bu yüzden Store yolu
MSIX'tir. Tauri MSIX üretmez (yalnız EXE ve MSI); paket bu depoda,
`makeappx` ile elde kurulur.

## Paketli sürümde değişenler

| Konu | Masaüstü kurulumu | Store paketi | Kod |
|---|---|---|---|
| Güncelleme | Uygulama içi imzalı güncelleyici | Store kurar; denetim, kurulum ve arka plan denetimi kapalı; Ayarlar › Güncellemeler bunu söyler | `src-tauri/src/updater.rs`, `update_source` |
| Açılışta başlama | `HKCU\...\Run` değeri, `--tepside` argümanı | Paketin başlangıç görevi (`MailhereBaslangic`); gizli başlatma etkinleştirme türünden anlaşılır | `src-tauri/src/autostart.rs` (`store_task`) |
| WebView2 | Kurucu indirir | Kurucu indiremez; yoksa açılışta söylenir (Windows 11'de ve güncel Windows 10'da hazır gelir) | `src-tauri/src/msix.rs` |

Paketli olup olmadığı `GetCurrentPackageFullName` ile anlaşılır
(`msix::packaged()`); ayrı derleme bayrağı yoktur.

**Açılışta başlama ilkesi korunur:** görev bildirimde kapalı gelir, uygulama
ilk çalıştırmada açar (masaüstü kurulumuyla aynı karar: `autostart::plan`).
Kullanıcı Ayarlar › Uygulamalar › Başlangıç'tan kapattıysa Windows uygulamanın
yeniden açmasına izin vermez; ayar ekranı hatayı oraya yönlendirerek söyler.

Veri klasörü: paket içinde `%LOCALAPPDATA%` yazımları Windows tarafından
pakete özel klasöre yönlendirilir
(`%LOCALAPPDATA%\Packages\<paket ailesi>\LocalCache\Local\com.furkanyildirim.mailhere`).
Uygulama kaldırılınca bu klasör silinir. Parolalar ve veritabanı anahtarı
Windows Kimlik Bilgileri Yöneticisi'nde durur (yönlendirilmez).

## Dosyalar

- `src-tauri/msix/AppxManifest.xml` — paket bildirimi şablonu (`{{KIMLIK}}`,
  `{{YAYINCI}}`, `{{YAYINCI_ADI}}`, `{{AD}}`, `{{SURUM}}`).
- `src-tauri/msix/Assets/` — logo varlıkları; `node scripts/msix-varliklar.mjs`
  `src-tauri/icons/app-icon.svg` dosyasından üretir (ikon değişince yeniden
  çalıştırılır, çıktı depoya girer).
- `scripts/msix-paketle.ps1` — düzen klasörü, `makepri` (kaynak dizini),
  `makeappx pack`. Paket imzalanmaz.
- `scripts/msix-dene.ps1` — paketi kendinden imzalı geçici sertifikayla
  imzalayıp kurar, açar, günlükten doğrular, kaldırır.
- `.github/workflows/magaza.yml` — derler, paketler, Windows'ta dener; paketi
  iş çıktısı olarak verir, istenirse sürüme ekler.

## Kimlik değerleri (bir kez)

Partner Center › Mailhere › **Ürün kimliği** sayfasındaki üç değer GitHub'da
depo **değişkeni** olarak girilir (Settings › Secrets and variables › Actions
› Variables). Gizli değildir ama birebir aynı olmalıdır (büyük-küçük harf,
boşluk, noktalama):

| Depo değişkeni | Partner Center'daki alan | Örnek biçim |
|---|---|---|
| `STORE_KIMLIK` | Package/Identity/Name | `12345Yayinci.Mailhere` |
| `STORE_YAYINCI` | Package/Identity/Publisher | `CN=XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX` |
| `STORE_YAYINCI_ADI` | Package/Properties/PublisherDisplayName | `Furkan YILDIRIM` |
| `STORE_AD` (isteğe bağlı) | Ayrılan ad | `Mailhere` |

Değişkenler tanımlı değilken iş **deneme kimliğiyle** paketler
(`Mailhere.Deneme`, dosya adı `-deneme.msix`): hat sınanır ama o paketi Store
kabul etmez.

## Paketi üretmek

Actions › **Mağaza paketi** › Run workflow (dal `main`). "surum" boş
bırakılırsa paket yalnız iş çıktısıdır (`msix`); `v0.11.1` gibi bir etiket
verilirse paket o sürüme de eklenir (yalnız gerçek kimlikle).

Yerelde (Windows, PowerShell 7, Windows SDK):

```powershell
pnpm install
pnpm tauri build --no-bundle
pwsh scripts/msix-paketle.ps1 -Exe target\release\mailhere.exe -Surum 0.11.0
pwsh scripts/msix-dene.ps1 -Paket target\msix\Mailhere_0.11.0_x64-deneme.msix -Kimlik Mailhere.Deneme -Yayinci 'CN=Mailhere Deneme' -Kalsin
```

`-Kalsin` uygulamayı ve deneme sertifikasını kurulu bırakır (elle bakmak
için); kaldırmak için Ayarlar › Uygulamalar'dan Mailhere'yi kaldır.

Paket sürümü `X.Y.Z.0` olur (dördüncü hane Store'a ayrılmıştır). Store aynı
sürümü ikinci kez kabul etmez: her gönderimde sürüm artmalıdır.

## İlk gönderim (elle, Partner Center)

1. Paketler › `.msix` dosyasını yükle. Partner Center kimlik ve sürümü
   doğrular; `runFullTrust` için kısa bir gerekçe ister: "Masaüstü e-posta
   istemcisi; sistem tepsisi, yerel şifreli veritabanı ve işletim sistemi
   anahtar zinciri için tam güvenli çalışır."
2. Mağaza girişi (Türkçe ve İngilizce): açıklama, ekran görüntüleri (en az
   bir; 1366×768 ya da büyük), kategori (Üretkenlik).
3. Özellikler: gizlilik politikası adresi (e-posta hesabına eriştiği için
   zorunlu), destek bağlantısı.
4. Yaş derecelendirmesi anketi.
5. Fiyat ve kullanılabilirlik: ücretsiz, pazarlar.
6. Gönder. Onaydan sonra sonraki sürümler aynı yoldan yeni paketle gider.

## Elle bakılacaklar (gerçek Windows'ta)

Actions testi kurulumu, açılışı, Mağaza kipini ve başlangıç görevinin
açıldığını doğrular. Şunlar ancak gerçek oturumda görülür:

- Oturum açılınca Mailhere pencere açmadan tepside başlıyor mu.
- Tepsi simgesi, bildirim kartı, kartın konumu.
- Ayarlar › Uygulamalar › Başlangıç'ta Mailhere görünüyor; oradan kapatınca
  uygulamadaki anahtar kapalı görünüyor ve açmayı deneyince yönlendirme çıkıyor.
- Google / Microsoft ile giriş (tarayıcıdan yerel adrese dönüş).
- Ek açma ve kaydetme.
- Masaüstü kurulumu (NSIS/MSI) aynı bilgisayarda kuruluysa önce kaldır: iki
  kurulum aynı anda açılışta başlamaya çalışır (ikincisi tek örnek kuralıyla
  kapanır) ve verileri ayrıdır.

## Bilinen sınırlar

- Yalnız x64 paketi üretilir (ARM64 Windows x64 öykünmesiyle çalıştırır).
- Masaüstü kurulumundaki veriler Store sürümüne kendiliğinden taşınmaz;
  hesaplar yeniden eklenir ya da Cihazlar ve aktarım (QR) kullanılır.
- `mailto:` bağlantıları için Mailhere varsayılan posta uygulaması olarak
  kaydedilmez (bildirimde protokol yok).
