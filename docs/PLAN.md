# Mailhere — plan

Ayrıntılı plan belgesi: [Mailhere — proje planı](https://claude.ai/code/artifact/65beb2e3-f640-4ebd-9efa-9bb15ba2faaf). Bu dosya onun depodaki özetidir.

## Onaylanan kararlar (2026-10-13)

| Karar | Seçim |
| --- | --- |
| Tasarım | tuvaldeki 8 ekran ve [`TASARIM.md`](TASARIM.md) |
| Yığın | Tauri 2 · Rust (tokio) · Svelte 5 + TypeScript + Vite · SQLite + FTS5 · işletim sistemi anahtar zinciri |
| Lisans | MIT |
| Ad | Mailhere |
| Dil | Türkçe (varsayılan) ve İngilizce |
| Bildirim varsayılanı | kart yalnız Kişiler kategorisinde; diğerleri tepsi rozeti |
| Gmail / Outlook | OAuth 2 (uygulama kaydı sonra); o zamana kadar uygulama parolasıyla IMAP |
| Windows imzası | ilk betada imzasız |
| Git | `main` dalı |

## Hedefler

| Hedef | Eşik |
| --- | --- |
| Açılış | 50.000 postalık kutuda ilk liste 1 sn altında |
| Arama | 100.000 postada yazarken 50 ms altında |
| Kaydırma | 10.000 satırda 60 fps |
| Boştaki bellek | 3 hesap, tepside 150 MB altı |
| Kurulum | 20 MB altı |
| Bildirim gecikmesi | sunucudan karta 3 sn altı |

## Yol haritası

| Aşama | Teslim |
| --- | --- |
| 0 · İskelet | Tauri + Svelte kabuk, tasarım jetonları, CI, boş paketler |
| 1 · Okuma | IMAP hesap + otomatik ayar, eşitleme, liste, okuma, güvenli HTML, arama |
| 2 · Yazma | SMTP, yanıt / ilet, taslak, ek, imza, geri al, giden kutusu |
| 3 · Bildirim | IDLE, köşe kartı, hızlı okuma, tepsi, sessiz saatler → **ilk deneme sürümü** |
| 4 · Akıllı kutu | akıllı süzgeç, ertele, sonra gönder, komut paleti |
| 5 · Sağlayıcılar | Gmail / Outlook OAuth, JMAP (Stalwart) |
| 6 · Yayın | Windows ve Linux paketleri, imzalı otomatik güncelleme → **0.1.0 beta** |

## Durum (2026-10-13)

| Aşama | Durum |
| --- | --- |
| 0 · İskelet | tamam |
| 1 · Okuma | tamam (IMAP; Gmail "Tüm Postalar" eşitlenmez, arşiv hedefi olarak kullanılır) |
| 2 · Yazma | tamam; eksik: taslağın sunucuya kaydı, imza düzenleme ekranı, iletilen postanın eklerini taşıma |
| 3 · Bildirim | tamam; GNOME Wayland'de sistem bildirimine düşüş gerçek masaüstünde denenecek |
| 4 · Akıllı kutu | tamam |
| 5 · Sağlayıcılar | başlamadı (OAuth uygulama kaydı ve JMAP eşitleme) |
| 6 · Yayın | Linux paketleri yerelde üretildi; imzalı otomatik güncelleme yok |

Açık işler: geçersiz sertifika için onay ekranı, çevrimdışı sunucu eylemlerinin kalıcı kuyruğu, tam ekran algılama.

İlk sürüm dışında: takvim, PGP ve S/MIME, Exchange'e özel protokol, mobil.
