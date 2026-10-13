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
| 2 · Yazma | tamam (sunucuda taslak, imza düzenleme, iletilen ekler dahil) |
| 3 · Bildirim | tamam (tam ekran algılama: Windows, Linux X11); GNOME Wayland'de sistem bildirimine düşüş gerçek masaüstünde denenecek |
| 4 · Akıllı kutu | tamam |
| 5 · Sağlayıcılar | kod tamam: Google / Microsoft OAuth (Dovecot XOAUTH2 ile sınandı), JMAP (Stalwart 0.16 ile sınandı). Gerçek Google / Microsoft girişi için uygulama kaydı ve istemci kimlikleri bekleniyor |
| 6 · Yayın | CI her push'ta Linux (deb, rpm, AppImage) ve Windows (NSIS, MSI) paketi üretir; imzalı güncelleme ve `latest.json` hazır, imza anahtarı depo gizli değerlerine eklenince sürüm imzalanır |

Sağlamlık: kalıcı sunucu işlem kuyruğu (yeniden başlatmaya dayanır), yarıda kalan eşitlemede atlanan posta yok, geri alınan taşımada hayalet ya da kopya satır yok, onaylı (sabitlenmiş) sertifikalar IMAP, SMTP ve JMAP'te.

Açık işler: Google `mail.google.com` kapsamı için Google uygulama doğrulaması; MSI yalnız İngilizce; güncelleme indirme ilerlemesi gösterilmiyor; Windows paketi gerçek bir Windows'ta elle denenecek.

İlk sürüm dışında: takvim, PGP ve S/MIME, Exchange'e özel protokol, mobil.
