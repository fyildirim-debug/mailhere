# Mailhere — program kuralları

Mailhere; Windows ve Linux için hızlı, sakin, klavyeyle yönetilen bir masaüstü e-posta istemcisidir; Android sürümü aynı çekirdekten ayrı bir depoda derlenir. Bu dosya projede çalışan herkes (insan ya da ajan) için bağlayıcıdır.

## Ürün ilkeleri (bozulmaz)

- **Posta hızlı gelir.** Telefonda Mailhere arka planda postaları sürekli denetler, yeni posta gelince hemen bildirir; bilgisayarda arka planda (pencere kapalıyken de) çalışır ve hemen bildirir. En hızlı yol kullanılır: IMAP IDLE, JMAP anlık akış, Android ön plan hizmeti; yoklama yalnız yedektir.
- **Açılışta başlar.** Bilgisayar ve telefon açılınca Mailhere kendiliğinden başlar. Bilgisayarda pencere açılmaz, yalnız sistem tepsisinde ikon olarak görünür; telefonda görünmez, arka planda başlayıp posta denetler. Kullanıcı ayardan kapatabilir.
- **Parola kilidi isteğe bağlıdır.** Posta kutusu kullanıcı isterse parolayla kilitlenir ve şifrelenir; açıkken girişte parola sorulur. Varsayılan kapalıdır; kapalıyken hiçbir akışta parola sorulmaz.
- Yeni her özellik bu üç ilkeyle uyumlu tasarlanır; birini zayıflatan değişiklik yapılmaz.

## Temiz oda

- Thunderbird, Outlook, Spark ya da başka bir istemciden **kod, görsel, ikon, metin, şablon, veri listesi** alınmaz; tersine mühendislik yapılmaz.
- Açık lisanslı (MIT / Apache-2.0 / BSD / OFL) kitaplık kullanmak serbesttir; GPL/AGPL kod uygulamaya bağlanmaz (test için ayrı süreç olarak çalıştırmak serbest).
- Uygulama kodu, arayüz, ikonlar ve metinler bu depoda sıfırdan yazılır.

## Tasarım dili (bağlayıcı)

Tam kurallar: [`docs/TASARIM.md`](docs/TASARIM.md). Tek doğruluk kaynağı: `app/src/styles/tokens.css`. Özet:

- Renk yalnız jetonla: `var(--ground|surface|paper|raised|line|line-strong|ink|ink-2|ink-3|accent|accent-ink|accent-soft|accent-text|sel|hover|amber|amber-text|ok|ok-text|danger)`. Bileşende ham hex yazılmaz. Uyarı ve başarı **metni** `--amber-text` / `--ok-text` ile (her zeminde ≥ 4.5:1); `--amber` / `--ok` yalnız ikon, nokta ve yıldızda.
- Devre dışı öğe yalnız `opacity: var(--dim-disabled)` ile soluklaşır; başka saydamlık değeri yazılmaz.
- Tek vurgu rengi; yalnız birincil eylem, seçim, odak ve okunmamış işareti.
- Yazı: yalnız Geist, yalnız **12 / 15 / 19 / 24 / 30** px. Ara boyut yok.
- Boşluk 4 px tabanlı; köşe 5–16, etiket 999; derinlik ya kenar ya gölge.
- İkon yalnız `Icon.svelte` + `icons.ts` (1.6 çizgi, 24 ızgara). Emoji, unicode sembol, dış ikon kitaplığı yok.
- Hareket: 120 / 200 / 280 ms, `cubic-bezier(.16,1,.3,1)`; tek yazılmış an bildirim kartının gelişi (520 ms). `prefers-reduced-motion` desteklenir.
- Yasak: degrade, cam/blur süsü, renkli sol kenar çizgisi, iç içe kart, sıfır bulanıklıklı blok gölge, gri metin renkli zeminde.
- Her etkileşimli öğe gerçek `button` / `a` / `input`; ikon düğmede `aria-label`; görünür `:focus-visible`.
- Boş durum ve hata metni: ne oldu + ne yapılabilir.

## Dil

- Arayüz metinleri, kod yorumları, commit mesajları **Türkçe**. Teknik terim İngilizce kalabilir.
- Arayüz metni bileşende sabit yazılmaz; `app/src/lib/i18n/tr.ts` ve `en.ts` sözlüklerine girer.

## Mimari

İki depo, tek çekirdek:

- **`mailhere`** (bu depo) — ortak çekirdek (`crates/*`, `app/`) ve masaüstü kabuğu (`src-tauri/`).
- **`mailheremobile`** — Android kabuğu. Bu depoyu `mailhere/` git alt modülü olarak (sabit commit) içerir; kendi `Cargo.lock`, `pnpm-lock.yaml`, `src-tauri/` (paket `mailhere-android`, `#[tauri::mobile_entry_point]`, Kotlin eklenti) ve `gen/android/` dizini vardır. Arayüzü doğrudan alt modülün `app/dist` çıktısından alır.

Modüller:

- `app/` — Svelte 5 + TypeScript + Vite arayüzü. Tek giriş, pencere türü `#/main`, `#/notify`, `#/quick/:id`, `#/compose`, `#/onboarding` ile seçilir. Kabuk `desktop` ya da `mobile` (`app/src/lib/shell.ts`); mobil bileşenler `app/src/windows/mobile/` altında tembel yüklenir.
- `crates/mh-app` — ortak uygulama katmanı: Tauri komutları (`commands`, `cmd_*`), `AppState` + `setup_core` (depo, eşitleyiciler, giden kutusu, zamanlayıcılar), olay köprüsü (`sink`), kurallar, elek, hatırlatmalar, `routes` (pencere rotaları ve adres denetimi), `files` (seçilmiş ek yolları), `notify` (ortak bildirim metni), `logging`. Kabuğa değil Tauri'ye bağlıdır; `keyring`, `single-instance`, `updater`, `tray-icon` **içermez**. Ortak komut listesi tek kaynaktan gelir: `shared_commands!` makrosu, `SHARED_COMMANDS` sabiti ve `crates/mh-app/commands.txt` (iki kabuğun `build.rs`'i `include_str!` ile okur; birim testi üçünü eşler).
- `crates/mh-app/src/platform.rs` — **`Platform` trait'i**: kabuğa özgü işler (`notify`, `counts_changed`, `open_main`, `open_quick`, `open_compose`, `open_file`, `save_file`, `pick_files`, `fatal`). Masaüstü `src-tauri/src/platform.rs` (`DesktopPlatform`: kart/tepsi/pencereler/dosya penceresi), Android `mailheremobile` (`AndroidPlatform`: sistem bildirimi, tek pencere + `mh:navigate`, paylaşım/SAF). Ortak kod pencere, tepsi ya da dosya penceresine doğrudan dokunmaz; `state.platform` üzerinden gider.
- `src-tauri/` — masaüstü kabuğu: pencereler, tepsi, bildirim kartı penceresi, anahtar zinciri (`KeyringSecrets`), dosya seçme penceresi, imzalı güncelleme, tek örnek; masaüstüne özel komutlar (`open_quick`, `notify_*`, `demo_notify`, `update_*`; tam liste `src-tauri/commands.txt`).
- `crates/mh-transfer` — cihazlar arası QR aktarımı (kripto + tel protokolü + sunucu/istemci; Store ve Tauri bilmez).
- `crates/mh-core` — ortak tipler (arayüzle sözleşme; `app/src/lib/api/types.ts` ile birebir aynı tutulur).
- `crates/mh-store` — SQLite + FTS5 yerel depo.
- `crates/mh-mime` — ayrıştırma, HTML temizleme, önizleme, akıllı kategori.
- `crates/mh-sync` — hesap ayarı keşfi, IMAP eşitleme, IDLE, sertifika onayı, kimlik bilgileri (parola / OAuth jetonu).
- `crates/mh-jmap` — JMAP (Stalwart vb.): keşif, eşitleme, anlık bildirim (EventSource), gönderim, taslak.
- `crates/mh-oauth` — Google ve Microsoft OAuth 2 (PKCE, yerel dönüş adresi, XOAUTH2).
- `crates/mh-send` — SMTP / JMAP gönderim, giden kutusu, geri alma, taslaklar, iletilen ekler.
- `crates/mh-notify` — bildirim kuralları.
- Arayüz yalnız gösterir ve komut gönderir; ağ, ayrıştırma ve veri işi Rust'tadır.
- Arayüz, Tauri dışında (tarayıcı, testler) `app/src/lib/api/mock.ts` sahte arka ucuyla çalışır; sözleşme değişirse ikisi birlikte güncellenir.

## Mobil kuralları

- Ortak kod (`crates/mh-app`, `app/src/lib`) platform ayrımı yapmaz; farklı davranış `Platform` trait'inde ya da kabuk crate'inde yaşar. `mh-app`'te `cfg(desktop)` / `cfg(mobile)` kullanılmaz (bu takma adları yalnız kabukların `build.rs`'i üretir).
- Yeni ortak komut üç yere birden girer: `shared_commands!` listesi (tek kaynaktan `SHARED_COMMANDS` da üretilir), `crates/mh-app/commands.txt`, her iki kabuğun `capabilities/*.json` dosyası. Yalnız bir kabuğa ait komut o kabuğun crate'inde kalır.
- `mh-app` ve `crates/*` her değişiklikte `aarch64-linux-android` için de clippy'den geçer (CI `android` işi). Android'de olmayan şey (keyring, tepsi, güncelleyici, X11) ortak koda girmez.
- Mobil arayüz `docs/TASARIM.md` §12'ye uyar; mobil i18n anahtarları `mob.*`.
- Gizli değerler Android'de `AndroidKeyStore` destekli kasada (`SecretVault.kt`); yedeklemeye kapalı. Bkz. `docs/GUVENLIK.md` "Android".

## Güvenlik

- Parola ve jetonlar yalnız işletim sistemi anahtar zincirinde (`keyring`); veritabanına, günlüğe, hata mesajına yazılmaz.
- Yerel veritabanı diskte SQLCipher ile şifreli (AES-256); 256 bitlik anahtar ilk açılışta üretilir ve yalnız anahtar zincirinde durur (`Mailhere` / `db-key`). Anahtar zinciri yoksa şifresiz veritabanına düşülmez; hata gösterilir. Veri klasörü 0700, veritabanı dosyaları 0600; `secure_delete` açık.
- HTML posta Rust'ta temizlenir (ammonia), betiksiz ve CSP'li çerçevede gösterilir; uzak içerik varsayılan kapalı.
- TLS zorunlu; geçersiz sertifika açık onay ister.

## Komutlar

```bash
pnpm install                 # arayüz bağımlılıkları
pnpm dev                     # tarayıcıda arayüz (sahte arka uç)
pnpm tauri dev               # masaüstü uygulaması
pnpm check                   # svelte-check + tsc
pnpm test                    # Vitest
pnpm e2e                     # Playwright (sahte arka uç)
cargo test --workspace       # Rust testleri
cargo clippy --workspace --all-targets -- -D warnings
pnpm tauri build             # paketler (Linux: deb, AppImage, rpm · Windows: nsis, msi)
```

Android (çekirdek denetimi bu depoda; paket `mailheremobile` deposunda):

```bash
rustup target add aarch64-linux-android
# NDK 28.x: CC/AR/RANLIB/LINKER değişkenleri NDK'nın clang'ına gösterilir (SQLCipher'in OpenSSL'i kaynaktan derlenir)
cargo clippy --locked --target aarch64-linux-android -p mh-app -- -D warnings   # CI "android" işi
# mailheremobile içinde:
pnpm tauri android dev                      # cihaz ya da emülatörde
pnpm tauri android build --apk --target aarch64
```

## Git

- Dal: `main`. Commit mesajı Türkçe, ne değiştiğini söyler.
- Yazar yalnız **Furkan YILDIRIM <posta@furkanyildirim.com>**. Commit ve PR metnine `Co-Authored-By`, `Claude-Session`, "Generated with" ya da benzeri imza **eklenmez**.
- Push'tan önce: `pnpm check`, `pnpm test`, `cargo test --workspace`, `cargo clippy` temiz.
