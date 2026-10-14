# Mailhere tasarım dili

Bu belge Mailhere arayüzünün **bağlayıcı** kurallarıdır. Yeni ekran, bileşen ya da değişiklik bu kurallara uyar; uymuyorsa önce bu belge güncellenir, sonra kod. Onaylı görsel kaynak: [tasarım tuvali](https://claude.ai/artifact/K3xuhJXmSEJnDgSDdrLkrP) (Ekim 2026, taslak 1, onaylandı).

Kodda tek doğruluk kaynağı `app/src/styles/tokens.css` dosyasıdır. Bu belgedeki değerler oradaki jetonlarla birebir aynıdır.

## 1. İlkeler

1. **Sakin zemin, tek vurgu.** Vurgu rengi yalnız birincil eylemde, seçimde, odak halkasında ve okunmamış işaretinde görünür. Süs için kullanılmaz.
2. **Hiyerarşi ağırlık ve boşlukla kurulur**, renk ve kutuyla değil. Kart, ancak gerçekten ayrı bir nesneyse (bildirim kartı, ek dosyası, menü) kullanılır; iç içe kart yoktur.
3. **Hızlı hissettirir.** Kullanıcı hiçbir zaman bir animasyonu beklemez. Veri yerelden gelir, iskelet ekran ancak gerçekten ağ beklenirken görünür.
4. **Klavye önce.** Her sık eylemin tek tuşu vardır ve arayüzde (ipucu, kısayol çipi) görünür.
5. **Gün boyu açık kalır.** Parlak beyaz alanlar sınırlı, metin karşıtlığı yüksek ama sert değil; koyu tema ayrı tasarlanmıştır, ters çevrilmemiştir.

## 2. Renk rolleri

Bileşenler **asla** ham renk yazmaz; yalnız aşağıdaki jetonları (`var(--…)`) kullanır. İstisna: avatar ve dosya türü paletleri (bkz. 2.3) ve platform pencere düğmesindeki kapat kırmızısı.

### 2.1 Açık tema

| Jeton | Rol | Değer |
| --- | --- | --- |
| `--ground` | pencere zemini, başlık çubuğu, kenar çubuğu | `#ECEEF2` |
| `--surface` | posta listesi, ikincil alanlar | `#F6F7F9` |
| `--paper` | okuma alanı, girdiler, kartlar | `#FFFFFF` |
| `--raised` | menü, açılır pencere, satır eylemleri | `#FFFFFF` |
| `--line` | ince ayraç | `#E1E4EA` |
| `--line-strong` | girdi ve düğme kenarı | `#D2D6DE` |
| `--ink` | birincil metin | `#15171C` |
| `--ink-2` | ikincil metin, ikonlar | `#4F5562` |
| `--ink-3` | üst bilgi, saat, sayaç, yer tutucu | `#636A76` |
| `--accent` | birincil eylem, okunmamış noktası, odak | `#4655D4` |
| `--accent-ink` | vurgu üstündeki metin | `#FFFFFF` |
| `--accent-soft` | etiket zemini, odak gölgesi, seçili metin | `#E6E8FB` |
| `--accent-text` | vurgu renkli metin ve bağlantı | `#3A47B8` |
| `--sel` | seçili liste satırı | `#E4E7FA` |
| `--hover` | üzerine gelme katmanı | `rgba(21,23,28,.045)` |
| `--amber` | yıldız, erteleme | `#C27D12` |
| `--ok` | başarı | `#2E8A57` |
| `--danger` | hata, silme onayı | `#C2372E` |
| `--brand` | marka bölgesi (karşılama paneli); üstünde her temada beyaz metin | `#4655D4` (koyu: `#3440B0`) |
| `--signal` | logodaki "burada" noktası | `#F2B544` |
| `--mail-paper` | HTML postanın kâğıdı (posta kendi renkleriyle tasarlandığı için her temada beyaz) | `#FFFFFF` |

### 2.2 Koyu tema

| Jeton | Değer |
| --- | --- |
| `--ground` | `#0F1115` |
| `--surface` | `#14161B` |
| `--paper` | `#191C22` |
| `--raised` | `#22252D` |
| `--line` | `#262A32` |
| `--line-strong` | `#343944` |
| `--ink` | `#E6E8ED` |
| `--ink-2` | `#AEB4BF` |
| `--ink-3` | `#8E95A1` |
| `--accent` | `#8C96F6` |
| `--accent-ink` | `#0D0F1F` |
| `--accent-soft` | `#272C4D` |
| `--accent-text` | `#AAB2FA` |
| `--sel` | `#242945` |
| `--hover` | `rgba(230,232,237,.05)` |
| `--amber` | `#E2A84A` |
| `--ok` | `#5CC48A` |
| `--danger` | `#EF7A70` |

Kurallar:

- HTML postalar kendi açık renkleriyle tasarlandığı için koyu temada da beyaz kâğıt (`--mail-paper`) üzerinde, köşeli bir yüzeyde gösterilir; düz metin postalar temayı izler.
- Zemin katmanı sırası: `ground` → `surface` → `paper` → `raised`. Bir katman kendinden alttakinin üstüne oturur; tersi olmaz.
- `--ink-3` yalnız `surface`, `paper` ve `sel` üstünde kullanılır. `ground` üstünde üst bilgi için `--ink-2` kullanılır (karşıtlık).
- Metin çiftleri her iki temada en az **4.5:1**, büyük metin ve ikonlar en az **3:1** karşıtlık sağlar.
- Tema varsayılan olarak sistemi izler; kullanıcı Açık / Koyu / Sistem seçebilir. Kök elemanda `data-theme="light|dark"`.

### 2.3 Avatar ve dosya türü paletleri

Avatar; gönderici adresinden deterministik seçilen 9 tonlu paletten gelir (zemin + aynı tonda koyu metin, gri değil). Değerler `app/src/lib/ui/palette.ts` içindedir. Dosya türü rozetleri: PDF kırmızı tonu, tablo yeşil tonu, diğerleri nötr.

## 3. Yazı

- Tek aile: **Geist** (OFL), 400 / 500 / 600. Uygulamayla birlikte gelir (`app/src/assets/fonts`), ağdan yüklenmez. Yedek: `ui-sans-serif, system-ui, sans-serif`.
- Ölçek yalnız beş adım; ara boyut **yasak**:

| Boyut | Rol | Ağırlık / satır |
| --- | --- | --- |
| 30 | karşılama başlığı (hesap ekleme) | 600 · 1.15 · -0.025em |
| 24 | posta konusu, pencere başlığı | 600 · 1.25 · -0.02em |
| 19 | bölüm başlığı (liste başlığı, hızlı okuma konusu) | 600 · 1.3 · -0.015em |
| 15 | gövde, liste, düğme, girdi | 400 / 500 / 600 · 1.45 (okuma 1.65) |
| 12 | saat, sayaç, etiket, üst bilgi, kısayol çipi | 400 / 500 / 600 |

- Okunmamış satırda gönderen 600, konu 500 + `--ink`; okunmuşta gönderen 500, konu 400 + `--ink-2`.
- Okuma gövdesi en fazla **68ch** genişlikte, satır aralığı 1.65.
- Sayılar (saat, sayaç, boyut) `font-variant-numeric: tabular-nums`. Başlıklar `text-wrap: balance`.
- Harf aralığı -0.025em'den sıkı olmaz.

## 4. Boşluk, köşe, derinlik

- Boşluk tabanı **4 px**: 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 40. Gruplar sıkı, gruplar arası cömert.
- Köşe yarıçapları: çip/kısayol 5 · küçük düğme 7 · ikon düğme 8 · düğme 9 · liste satırı, girdi 10 · ek kartı, toast 12 · bildirim kartı, palet 14–16 · etiket 999.
- Derinlik bir kez söylenir: **ya kenar ya gölge**. Gölgeler her zaman ofset + yumuşak bulanıklık taşır:
  - `--shadow-sm`: satır eylemleri, seçili segment
  - `--shadow`: menü, toast
  - `--shadow-lg`: bildirim kartı, komut paleti
- Cam/bulanık arka plan, degrade, renkli sol kenar çizgisi, sıfır bulanıklıklı blok gölge **yasak**.

## 5. Bileşenler

| Bileşen | Kural |
| --- | --- |
| Birincil düğme | `--accent` zemin, `--accent-ink` metin, 36 px (küçük 32), köşe 9, 15/500. Ekranda tek birincil düğme. |
| Yumuşak düğme | `--paper` zemin, `--line-strong` kenar, 34 px. |
| İkon düğme | 34×34 (satır içinde 28×28), saydam zemin, `--ink-2` ikon; üzerine gelince `--hover` ve `--ink`. Her ikon düğmede `aria-label` ve kısayollu `title`. |
| Segment (akıllı süzgeç) | `--ground` iz, 3 px iç boşluk; seçili segment `--paper` + `--shadow-sm`. |
| Kenar çubuğu öğesi | 34 px, köşe 8; etkin öğe `--paper` zemin (koyuda `--raised`). Sayaç 12/500 sağda. |
| Liste satırı | 3 satır (gönderen+saat, konu, önizleme), 8 px okunmamış noktası, 34 px avatar; seçili `--sel`; üzerine gelince sağ üstte Arşivle / Ertele / Sil eylemleri saati örter. |
| Okuma alanı | araç çubuğu 52 px; konu 24/600; önceki iletiler tek satıra katlanır; gövde 68ch; ekler 252 px kartlar; altta hızlı yanıt kutusu. |
| Uzak içerik şeridi | koruma ikonu + "Uzak görseller ve izleme pikselleri engellendi" + "Bu göndericiye izin ver". |
| Toast | ters renk (`--ink` zemin), alt ortada, "Geri al" eylemi, 5 sn. |
| Bildirim kartı | 372 px, köşe 14, `--shadow-lg`; başlık satırı (logo, hesap, zaman, sessize al, kapat), avatar 40, gönderen / konu / 2 satır önizleme, eylemler Aç · Okundu say · Arşivle, altta 2 px geri sayım çizgisi (üzerine gelince durur). En çok 3 kart, fazlası "N posta daha" özeti. |
| Hızlı okuma | 440×700 pencere; başlık çubuğunda Üstte tut, Uygulamada aç, Kapat; altta hazır yanıt çipleri ve yanıt kutusu. |
| Yazma | Kimden / Kime (çipler) / Konu alanları, sade biçim çubuğu, "Gönder" + "Sonra gönder" bölünmüş düğmesi, gönderdikten sonra 10 sn geri alma. |
| Komut paleti | 620 px, üstte 19 px arama girdisi; bölümler Arama · Eylemler · Postalar · Kişiler; ok tuşları + Enter. |
| Girdi | 44 px (form) / 32 px (satır içi), `--line-strong` kenar; odakta `--accent` kenar + 3 px `--accent-soft` halka. |
| Kısayol çipi | 20–22 px, köşe 5, `--line-strong` kenar, 12/500. |

Boş durumlar ve hatalar her zaman iki şey söyler: **ne oldu** ve **ne yapılabilir**.

## 6. Hareket

| Süre | Kullanım |
| --- | --- |
| 120 ms | anlık geri bildirim (üzerine gelme, basma) |
| 200 ms | durum değişimi (seçim, sekme) |
| 180–280 ms | menü, toast, pencere içeriği |
| 520 ms | **bildirim kartının gelişi** — uygulamanın tek "yazılmış anı" |

- Eğri: `cubic-bezier(.16, 1, .3, 1)`. Çıkış girişten hızlıdır. Zıplama/elastik eğri yok.
- Yalnız `transform`, `opacity`, `filter` ve gölge canlandırılır; genişlik/yükseklik/konum canlandırılmaz.
- `prefers-reduced-motion: reduce` iken kayma ve bulanıklık kalkar; renk ve saydamlık geçişi kalır.

## 7. İkonlar

- Kendi çizimimiz: 24 px ızgara, **1.6** çizgi, yuvarlak uç ve birleşim, dolgu yok (yıldız dolu hali hariç). Tek kaynak `app/src/lib/ui/icons.ts`, tek bileşen `Icon.svelte`.
- Başka kütüphane ikonu, emoji ya da unicode sembolü ikon yerine kullanılmaz.
- Boyutlar: 14 (satır içi işaret), 16 (küçük düğme), 18 (araç çubuğu), 28–32 (boş durum).

## 8. Kısayollar

| Tuş | Eylem |
| --- | --- |
| C | Yeni posta |
| R / A / F | Yanıtla / Tümünü yanıtla / İlet |
| E | Arşivle |
| H | Ertele |
| S | Yıldızla |
| U | Okunmadı say |
| P | Sabitle / sabitlemeyi kaldır |
| Del | Sil |
| J / K | Sonraki / önceki posta |
| Shift J / Shift K | Seçimi sonraki / önceki satıra genişlet |
| Ctrl A | Görünen tüm satırları seç |
| Enter | Seçili postayı aç |
| Ctrl K | Komut paleti |
| Ctrl Enter | Gönder |
| Ctrl Z | Son işlemi geri al |
| Esc | Seçimi bırak / kapat / geri |

Kısayollar metin girdisinde odak varken tek harfli tuşlarda çalışmaz.

Çoklu seçim: Ctrl (Cmd) + tık satırı tek tek işaretler, Shift + tık aralık seçer; satırın avatarı üzerine gelince çıkan seçim düğmesi de aynı işi yapar. İşaretli satır `--sel` zemini alır. Birden çok satır işaretliyken okuma alanının yerine toplu işlem paneli gelir (sayı, Arşivle, Sil, Okundu/Okunmadı, Yıldızla, Ertele, Klasöre taşı, Etiketler, Sabitle, Seçimi bırak); her eylem sayılı bildirimle geri alınabilir.

Sabitleme ve etiketler yalnız bu bilgisayarda tutulur; sunucuya bayrak ya da klasör yazılmaz. Sabitlenenler gelen kutusu ve klasör görünümlerinde "Sabitlenenler" grubunda en üstte durur. Etiket rengi hesap rengi gibi kullanıcı verisidir: sınırlı paletten seçilir (`TAG_COLORS`), listede 8 px nokta, okuma alanında renkli noktalı çip olarak görünür.

## 9. Metin ve dil

- Arayüz Türkçe (varsayılan) ve İngilizce. Metinler `app/src/lib/i18n` sözlüklerindedir; bileşende sabit metin yazılmaz.
- Düğme yaptığı eylemi söyler ("Arşivle", "Hesabı ekle"); "Tamam", "Gönder" dışında genel sözcük yok.
- Sesli, kısa, sakin. Ünlem yok, emoji yok, teknik jargon yalnız ayarlarda.
- Türkçe büyük/küçük harf dönüşümü `toLocaleUpperCase('tr')` ile yapılır (i/İ, ı/I).

## 10. Erişilebilirlik

- Tüm etkileşimli öğeler gerçek `<button>`, `<a>`, `<input>`; tıklanabilir `div` yok.
- Görünür odak halkası: 2 px `--accent`, 2 px ofset (`:focus-visible`).
- Yalnız renkle bilgi verilmez (okunmamış: nokta + kalın yazı; yıldız: ikon + etiket).
- Dokunma/tıklama hedefi en az 28 px, birincil hedefler 34 px+.
- Seçim, imleç, kaydırma çubuğu ve odak halkası paletten temalandırılır.

## 11. Platform

- Pencereler kendi başlık çubuğunu çizer (46 px ana, 40–42 px ikincil); küçült / büyüt / kapat sağda, kapatın üzerine gelince `#C42B1C`.
- Uygulama kapatılınca tepside kalır; tepsi ikonunda okunmamış sayısı.
- Linux Wayland'de bildirim kartı konumlanamıyorsa işlem düğmeli sistem bildirimi kullanılır.

## 12. Mobil uyarlama

Android sürümü aynı arayüz kodundan (`app/`) derlenir; kabuk `mobile` olunca mobil bileşenler yüklenir. Yukarıdaki jeton, yazı, renk, ikon ve hareket kuralları **aynen** geçerlidir; bu bölüm yalnız ölçü ve gezinmeyi uyarlar.

**Ölçüler**

| Öğe | Mobil |
| --- | --- |
| Dokunma hedefi | en az 44×44, birincil eylem 48 |
| Liste satırı | 72 px (sıkı görünüm 60) |
| Uygulama çubuğu | 56 px |
| Alt eylem çubuğu (okuma) | 56 px; düğme hedefleri 44 |
| Yüzen "Yeni posta" düğmesi | 56 px, sağ alt, `--accent` + `--shadow`; ekranda tek birincil |
| İkon | masaüstü boyutlarına 22 eklenir (çubuk ve satır eylemleri) |
| Yazı | ölçek 12 / 15 / 19 / 24 / 30 değişmez; okumada konu **19** (masaüstünde 24) |
| Alt sayfa satırı | 48 px, köşe 14, `--raised` + `--shadow-lg` |
| Çekmece | sol, 300 px, `--ground` |

**Gezinme**

- Alt sekme çubuğu yok. Sol çekmece kenar çubuğunun karşılığıdır: gelen kutusu, elek, ertelenen, bekleyen, yıldızlı, taslak, gönderilen, arşiv, çöp; hesaplar ve klasör ağacı; etiketler; kayıtlı aramalar; Ayarlar.
- Yığın: liste → okuma → yazma; ayarlar yığına itilir. Her itme `history.pushState`; geri tuşu önce açık çekmece / alt sayfa / menüyü kapatır, sonra yığından çıkar, kökte uygulamayı kapatır.
- Menüler mobilde alt sayfa (`Sheet`) olarak açılır. Toast aynı kalır, alt çubuğun üstünde durur.
- Kaydırma: sola **Arşivle** (`--accent` zemin + ikon), sağa **Ertele** (`--amber`); eşik %40, 200 ms dönüş, yalnız `transform`; `prefers-reduced-motion` iken anlık. Uzun basış çoklu seçimi açar. Aşağı çekme eşitler.
- Okumada alt çubuk: Yanıtla · Tümünü yanıtla · İlet · Arşivle · Sil.
- Komut paleti ve klavye kısayolları yerine tam ekran arama; bildirim kartı, hızlı okuma ve pencere düğmeleri yoktur (işletim sistemi bildirimi kullanılır).

**Teknik**

- `index.html` görünüm alanı: `width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content`; güvenli alan `env(safe-area-inset-*)`.
- Koyu tema aynı `data-theme`; mobil i18n anahtarları `mob.*` önekiyle.
- Geçişler yalnız `transform` / `opacity`, 200–280 ms; mobil kabuk tembel yüklenir, listeler sanallaştırılır, Geist önyüklenir.
- Ölçüler `app/src/styles/mobile.css` değişkenlerinde tek yerde tutulur: `--m-appbar` 56, `--m-bar` 56, `--m-row` 72 / `--m-row-compact` 60, `--m-touch` 44 / `--m-touch-lg` 48, `--m-drawer` 300, `--m-fab` 56 (köşe 16); güvenli alan `--sat/--sab/--sal/--sar`. İkon boyutu çubuklarda 22, yüzen düğmede 24.
- Seçim kipinde uygulama çubuğu `--accent` zemin + `--accent-ink`. Örtü `color-mix(in srgb, var(--ink) 42%, transparent)`. Hareket: ekran itme 280 ms (`translateX(28px)` + opacity), çekmece 280 ms, kaydırma dönüşü ve alt çubuk 200 ms; hepsi `cubic-bezier(.16,1,.3,1)`.
- Kenardan kaydırma çekmeceyi açar (liste ekranında soldaki 20 px şerit); uzun basış 450 ms. Toast mobilde yüzen düğmenin üstünde durur.
