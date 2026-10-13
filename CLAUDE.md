# Mailhere — program kuralları

Mailhere; Windows ve Linux için hızlı, sakin, klavyeyle yönetilen bir masaüstü e-posta istemcisidir. Bu dosya projede çalışan herkes (insan ya da ajan) için bağlayıcıdır.

## Temiz oda

- Thunderbird, Outlook, Spark ya da başka bir istemciden **kod, görsel, ikon, metin, şablon, veri listesi** alınmaz; tersine mühendislik yapılmaz.
- Açık lisanslı (MIT / Apache-2.0 / BSD / OFL) kitaplık kullanmak serbesttir; GPL/AGPL kod uygulamaya bağlanmaz (test için ayrı süreç olarak çalıştırmak serbest).
- Uygulama kodu, arayüz, ikonlar ve metinler bu depoda sıfırdan yazılır.

## Tasarım dili (bağlayıcı)

Tam kurallar: [`docs/TASARIM.md`](docs/TASARIM.md). Tek doğruluk kaynağı: `app/src/styles/tokens.css`. Özet:

- Renk yalnız jetonla: `var(--ground|surface|paper|raised|line|line-strong|ink|ink-2|ink-3|accent|accent-ink|accent-soft|accent-text|sel|hover|amber|ok|danger)`. Bileşende ham hex yazılmaz.
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

- `app/` — Svelte 5 + TypeScript + Vite arayüzü. Tek giriş, pencere türü `#/main`, `#/notify`, `#/quick/:id`, `#/compose`, `#/onboarding` ile seçilir.
- `src-tauri/` — Tauri 2 kabuğu: pencereler, tepsi, komutlar, olaylar, imzalı güncelleme.
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

## Güvenlik

- Parola ve jetonlar yalnız işletim sistemi anahtar zincirinde (`keyring`); veritabanına, günlüğe, hata mesajına yazılmaz.
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

## Git

- Dal: `main`. Commit mesajı Türkçe, ne değiştiğini söyler.
- Yazar yalnız **Furkan YILDIRIM <posta@furkanyildirim.com>**. Commit ve PR metnine `Co-Authored-By`, `Claude-Session`, "Generated with" ya da benzeri imza **eklenmez**.
- Push'tan önce: `pnpm check`, `pnpm test`, `cargo test --workspace`, `cargo clippy` temiz.
