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
| `--ink-3` | üst bilgi, saat, sayaç, yer tutucu | `#5E6572` |
| `--accent` | birincil eylem, okunmamış noktası, odak | `#4655D4` |
| `--accent-ink` | vurgu üstündeki metin | `#FFFFFF` |
| `--accent-soft` | etiket zemini, odak gölgesi, seçili metin | `#E6E8FB` |
| `--accent-text` | vurgu renkli metin ve bağlantı | `#3A47B8` |
| `--sel` | seçili liste satırı | `#E4E7FA` |
| `--hover` | üzerine gelme katmanı | `rgba(21,23,28,.045)` |
| `--amber` | yıldız, erteleme, uyarı ikonu | `#C27D12` |
| `--amber-text` | amber uyarı metni | `#8F5A05` |
| `--ok` | başarı ikonu ve noktası | `#2E8A57` |
| `--ok-text` | başarı metni | `#237347` |
| `--danger` | hata, silme onayı | `#C2372E` |
| `--brand` | marka bölgesi (karşılama paneli); üstünde her temada beyaz metin | `#4655D4` (koyu: `#3440B0`) |
| `--signal` | logodaki "burada" noktası | `#F2B544` |
| `--mail-paper` | HTML postanın özgün kâğıdı: açık temada ve koyu temada "Özgün renkler" seçiliyken (posta kendi renkleriyle tasarlandığı için her temada beyaz) | `#FFFFFF` |
| `--qr-paper` / `--qr-ink` | QR kodun zemini ve modülleri; telefon kamerası ters (açık modül, koyu zemin) kodu güvenilir okumadığı için her temada beyaz üstüne koyu | `#FFFFFF` / `#15171C` |

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
| `--amber-text` | `#E2A84A` |
| `--ok` | `#5CC48A` |
| `--ok-text` | `#5CC48A` |
| `--danger` | `#EF7A70` |

Kurallar:

- Koyu temada HTML posta koyu temaya **eşlenir** (`app/src/lib/mail/darkmail.ts`), süzgeçle (`invert`) ters çevrilmez: açık zeminler `--paper` tonuna koyulaşır (beyaz tam `--paper`, açık griler biraz açık; ton korunur), koyu yazı `--ink` / `--ink-2`'ye, kenar çizgileri `--line` / `--line-strong`'a oturur. Görseller, zaten koyu ya da orta tonlu zeminler (başlık bandı, düğme) ve üstlerindeki yazı olduğu gibi kalır. Her postada "Özgün renkler" ikon düğmesi (`contrast`, `aria-pressed`) postayı beyaz kâğıtta (`--mail-paper`) kendi renkleriyle gösterir; "Postaları koyu temaya uyarla" ayarı kapalıysa postalar hep özgün renklerinde. Düz metin postalar temayı izler.
- QR kodu (cihazlar arası aktarım) koyu temada da beyaz kart (`--qr-paper`, köşe 12, `--line` kenar) üstünde `--qr-ink` modüllerle çizilir; 4 modüllük sessiz bölge kartın içindedir. Okunabilirlik temadan önce gelir; QR'ın kendisi hiçbir zaman tema rengine boyanmaz.
- Zemin katmanı sırası: `ground` → `surface` → `paper` → `raised`. Bir katman kendinden alttakinin üstüne oturur; tersi olmaz.
- `--ink-3` yalnız `surface`, `paper` ve `sel` üstünde kullanılır. `ground` üstünde üst bilgi için `--ink-2` kullanılır (karşıtlık).
- Uyarı ve başarı **metni** `--amber-text` / `--ok-text` ile yazılır (her zeminde ≥ 4.5:1); `--amber` / `--ok` yalnız ikon, nokta ve yıldızdadır (3:1 yeter). Koyu temada ikisi aynı tondur.
- Hesap rozetindeki harf hesap renginin mürekkeple koyulaşmışıdır: `color-mix(in srgb, var(--c) 60%, var(--ink))`, zemin aynı rengin %18'i; `--ink` temayla döndüğü için iki temada AA.
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
| Kenar çubuğu | varsayılan 248 px (ayırıcıyla 200–360), `--ground`; **geri çekilir**: pasif öğeler `--ink-2`, ikonlar 16 px, bölüm başlıklarındaki yönetim düğmeleri yalnız kenar çubuğu kullanılırken (üzerine gelme, klavye odağı) belirir, alt satır ayırıcısızdır. Sıra: Yeni posta (kısayol harfi düğmenin içinde, çipsiz) · çok hesapta **hesap kapsamı** ("Tüm hesaplar" + her hesap; 40 px iki satırlı: ad / alan adı, sağda okunmamış) · kapsama göre daralan görünümler · her hesabın başlığı altında yalnız **istenmeyen + özel klasörler** (rol klasörleri görünümlerle sunulduğu için yinelenmez; "Tüm hesaplar"da hesap grupları katlanır) · etiketler · kayıtlı aramalar; tek hesapta kapsam bölümü yoktur, hesap başlığı ad + alan adı taşır (ad kısalır, alan adı hiç kesilmez), "Hesap ekle" listenin sonunda. Kaydırma çubuğu ince ve sessizdir (`.scroll-quiet`: yalnız üzerine gelince ya da kaydırırken). Altta eşitleme durumu, Ayarlar ve "Kenar çubuğunu daralt" (Ctrl B). |
| İkon rayı | 56 px, `--ground`: Yeni posta (40 px `--accent` kare, ekranın tek birincil düğmesi) · "Kenar çubuğunu aç" (Ctrl B) · görünümler (40×38, ikon 18, etkin `--paper`/`--raised`) · altta eşitleme noktası ve Ayarlar. Sayaç ikonun sağ üstünde 12/600 `tabular-nums`, zemini düğmeninki (ikon çizgisini örter), 99'dan sonra "99+". Ad ve sayı `title` ve erişilebilir adda ("Gelen Kutusu · 3"). |
| Dar pencere | ≥ 1120 px kenar çubuğu · liste · okuma. 860–1120 px kenar çubuğu ikon rayına iner; Ctrl B tam kenar çubuğunu **katman** olarak açar (`--shadow-lg`, 200 ms soldan kayma + saydamlık, arkası `--ground` %40 örtü, bulanıklık yok); görünüm seçince, Esc ya da dışarı tıklayınca kapanır, odak ilk öğeye gider. Geniş pencerede Ctrl B kenar çubuğunu raya indirir (tercih kalıcı). < 860 px **yığın**: ray · liste YA DA okuma; posta açılınca (tık, Enter, palet, bildirim) okuma tam genişlik, araç çubuğunun başında "Listeye dön" (Esc); görünüm ya da arama değişince liste. Okuma alanı 680 px'ten darsa yanıt düğmeleri yalnız ikondur (kapsayıcı sorgusu). |
| Sütun ayırıcısı | sütunlar arasındaki 1 px `--line` çizgi, iki yanına taşan 9 px görünmez tutma alanı, `col-resize`; üzerine gelince `--line-strong`, sürüklerken ve klavye odağında `--accent`. `role="separator"`, `aria-valuenow/min/max` (soldaki sütunun genişliği); ←/→ 8 px, Home/End en dar/en geniş, Enter ya da çift tık varsayılan. Kenar çubuğu 200–360, liste 320–640 ve okuma alanına en az 440 px kalır. Genişlikler kişisel tercihtir (tarayıcı deposu). |
| Odak kipi | Shift O ya da palet ("Odak kipi"); listenin başlığındaki `focus` ikonlu düğme. Kenar çubuğu ve liste gizlenir, okuma alanı tam genişlik; 52 px üst çubukta "Odak" + 12/500 `tabular-nums` ilerleme ("3/17", `role="status"`) ve "Odaktan çık · Esc". Sağda 232 px `--surface` sütunda kısayol çipli dikey eylem listesi: Arşivle E (tek birincil, `--accent`), Ertele H, Yanıtla R, Sil #, Yıldızla S, Klasöre taşı V, Etiketle L · Okunmadı say U · Atla J, Önceki posta K. Listeden çıkaran eylemden sonra sıradaki işlenmemiş posta 120 ms saydamlıkla gelir; geri alma (toast, Ctrl Z) postayı kipe de geri getirir. Hepsi bitince boş durum ("Gelen kutusu bitti" + ne yapılabilir, Odaktan çık / Ertelenenleri aç). |
| Seçici (V / L) | 440 px, palet gibi (`--paper`, köşe 16, `--shadow-lg`); başlık 12/600 `--ink-3`, 40 px arama girdisi, 38 px satırlar (klasörler ağaç girintili, etiketler renk noktalı, takılı etiket `check`), seçili satır `--sel`; yazarak bulanık süzülür, eşleşen parça 600 + alt çizgi. Taşıma ve etiket mevcut komutlarla, geri alınabilir. Etiket yoksa boş durum + "Etiket oluştur". |
| Kısayol sayfası (?) | 920 px pencere, üç grup (Eylemler · Gezinme · Uygulama), her satırda kısayol çipleri ve açıklama; Esc ya da ? kapatır. Ayarlar › Güncellemeler › Klavye kısayolları aynı listeyi (`KEY_GROUPS`) gösterir. |
| Arama çipleri | arama sonuçlarının başlığı altında: Kimden, Tarih ve Hesap (çok hesapta) menülü; Ekli ve Okunmamış aç-kapa. 28 px, köşe 999, 12/500, `--line-strong` kenar; seçili `--accent-soft` + `--accent-text`, değerli çipte değer ve kaldır (×) düğmesi. Seçim sorgu metnine işleç olarak yazılır (`from:`, `after:`, `has:ek`, `is:okunmamış`, `hesap:`); çipin durumu metinden okunur. Arama başlığına basınca sorgu palette düzenlenir. |
| Kenar çubuğu öğesi | 34 px, köşe 8; etkin öğe `--paper` zemin (koyuda `--raised`). Sayaç 12/500 sağda, `tabular-nums`, tek sütunda hizalı. |
| Liste satırı | 3 satır (gönderen+saat, konu, önizleme), 8 px okunmamış noktası, 34 px avatar; seçili `--sel`; üzerine gelince sağ üstte Arşivle / Ertele / Sil eylemleri saati örter. |
| Okuma alanı | araç çubuğu 52 px (Arşivle · Ertele · Sil, 1×18 `--line` ayraç, Klasöre taşı · Okunmadı say · Sabitle · Etiketle); konu 24/600 (alan 680 px'ten darsa 19/600, tarih alıcı satırında); önceki iletiler tek satıra katlanır, tek iletide "1 ileti" satırı yoktur; seçili iletinin yanıtla / ilet düğmeleri yalnız araç çubuğundadır; gövde 68ch; ekler 252 px kartlar; altta hızlı yanıt kutusu: boş ve odaksızken tek satır, hazır yanıt çipleri, alıcılar ve Gönder odakla açılır. |
| Uzak içerik şeridi | koruma ikonu + "Uzak görseller ve izleme pikselleri engellendi" + "Bu göndericiye izin ver". Toplu gönderim şeridiyle ("Abonelikten çık") birlikteyse ikisi tek kutuda (`--surface`, `--line` kenar, köşe 10), aralarında 1 px `--line` çizgi. Gönderen satırında ad adresin kendisiyse adres yinelenmez. |
| Özgün renkler geçişi | gönderen satırında (hızlı okumada başlık çubuğunda) `contrast` ikonlu düğme; yalnız koyu temada ve HTML postada, `aria-pressed`; basılıyken `--accent-soft` zemin + `--accent-text`. Çerçeve yeniden yüklenmez, yalnız renkler değişir. |
| Toast | ters renk (`--ink` zemin), alt ortada, "Geri al" eylemi, 5 sn. Kısayol ipucu (masaüstü): metnin yanında soluk "Kısayol" + ters renkli kısayol çipi. |
| Bildirim kartı | 372 px, köşe 14, `--shadow-lg`; başlık satırı (logo, hesap, zaman, sessize al, kapat), avatar 40, gönderen / konu / 2 satır önizleme, eylemler Aç + seçilen en çok üç bildirim düğmesi (varsayılan Yanıtla · Sil · İstenmeyen), altta 2 px geri sayım çizgisi (üzerine gelince durur). Yerinde yanıt alanı, onay satırı ve hata: §5.1. En çok 3 kart, fazlası "N posta daha" özeti. |
| Hızlı okuma | 440×700 pencere; başlık çubuğunda Üstte tut, Uygulamada aç, Kapat; altta hazır yanıt çipleri ve yanıt kutusu. |
| Yazma | Kimden / Kime (çipler) / Konu alanları, sade biçim çubuğu, "Gönder" + "Sonra gönder" bölünmüş düğmesi (kısayol yalnız düğmedeki "Ctrl Enter" çipinde), gönderdikten sonra 10 sn geri alma. Mobilde Bilgi ve Gizli tek ikon düğmede (`chevronDown`, 44 px) birlikte açılır; telefon genişliğinde (≤ 420 px) imza seçici yalnız ikon, tam ad erişilebilir adda. Alıcı eklenince "En az bir alıcı ekle." kalkar. |
| Komut paleti | 620 px, üstte 19 px arama girdisi; bölümler Arama · Eylemler · Postalar · Kişiler; ok tuşları + Enter. Her komutun kısayolu sağda çip olarak yazar. Komutlar iki dilin adlarıyla ve eşanlamlılarla bulanık eşleşir (Türkçe harf ve aksan katlanır); boş palette sık kullanılanlar üstte. Eşleşen parça renkle değil 600 + alt çizgiyle. İşleç tamamlama: `from:` kişi, `after:` / `before:` hazır tarih (bugün, bu hafta, bu ay, son 30 gün), `hesap:` hesap önerir; değer boşken öneriler, yazılınca Enter arar, Tab ilk öneriyi yazar. |
| Girdi | 44 px (form) / 32 px (satır içi), `--line-strong` kenar; odakta `--accent` kenar + 3 px `--accent-soft` halka. |
| Kısayol çipi | 20–22 px, köşe 5, `--line-strong` kenar, 12/500. |
| Menü | `--raised`, köşe 12, `--shadow`, 36 px satırlar; etiket kadar genişler (220–440 px, kırpılmaz), konum pencereye 8 px payla sıkıştırılır. Kısayol sağda kısayol çipiyle, sayı / adres / tarih soluk 12 px metinle. Mobilde alt sayfa. |
| Devre dışı | saydamlık tek değer: `--dim-disabled` (0.45); menü satırında saydamlık yerine `--ink-3` metin. |
| Boş durum | ikon 56 px `--accent-soft` disk içinde (`--accent-text`, 26 px), başlık 19/600, gövde `--ink-3` en çok 32ch; liste ve mobil ekranlarda aynı. Tek tanım `global.css` (`.empty`, `.empty-icon`, `.empty-title`, `.empty-body`). Sessiz yer tutucu (`.empty.quiet`: okuma alanı "Okumak için bir posta seç", kurallar, planlananlar, seçici) disksiz simge ve 15 px başlık taşır. Metin görünüme göre konuşur: arama sonucu "Sonuç yok", Ertelenenler, Yıldızlılar, Taslaklar, Gönderilenler, Arşiv, Çöp ve klasörün kendi metni var; gelen kutusu "Burası sakin". |

Boş durumlar ve hatalar her zaman iki şey söyler: **ne oldu** ve **ne yapılabilir**.

### 5.1 Bildirim kartı: düğmeler, yanıt, onay

- **Düğmeler.** Birincil eylem "Aç" (doğrulama kodu postasında "Kodu kopyala" birincil, "Aç" ikincil). Yanında Ayarlar › Bildirimler › **Bildirim düğmeleri**'nde seçilen en çok üç düğme, sabit sırayla: Yanıtla · Okundu · Arşivle · Sil · İstenmeyen (varsayılan Yanıtla, Sil, İstenmeyen; hiçbiri seçilmezse yalnız "Aç"). Sıra kartın tam iç genişliğinde, tek satır, 32 px yumuşak düğmeler ve kısa etiketler; erişilebilir ad ve ipucu eylemi tam söyler ("Okundu say", "İstenmeyen olarak işaretle"), görünen etiketi içerir. Etiketler sığmazsa (kod kartı, uzun dil) seçilen düğmeler sağa toplanıp 32×32 ikon düğmeye iner (`reply`, `check`, `archive`, `trash`, `spam`); taşma ve kırpma yoktur. Android aynı seçimi bildirim düğmesi olarak gösterir; seçim cihaza özgüdür, aktarımla taşınmaz.
- **Yanıt alanı.** "Yanıtla" kartı yerinde büyütür (pencere yüksekliği izler; kart penceresi odak çalmadan açılır, "Yanıtla"ya basınca odak alır): "Kime: …" 12 `--ink-2` (postada Reply-To varsa o), odaklanmış kutu 1 satırdan 4 satıra büyür (sonra içinde kayar), altta soluk "Ctrl Enter" ipucu, Vazgeç ve Gönder (kartın tek birincil düğmesi). Ctrl/Cmd Enter gönderir, Esc ve Vazgeç kapatır (yazılan kartta kalır, odak "Yanıtla"ya döner). Alan açıkken geri sayım yoktur, kart kendiliğinden kapanmaz. 200 ms saydamlık + 4 px kayma.
- **Onay satırı.** Eylemden sonra kart tek satıra iner (başlık ve önizleme kalkar): `--ok` ikon (`check`; yanıtta `send`), 15/500 ne olduğu ("Silindi", "Arşivlendi", "Okundu sayıldı", "İstenmeyen'e taşındı") ve 12 `--ink-3` "gönderen · konu", "Geri al" (yumuşak, küçük) ve Kapat. 5 sn sonra kart kapanır (üzerine gelince durur). Geri al taşınanı gelen kutusuna, okunduyu okunmamışa döndürür ve kartı posta haline (geri sayım baştan) getirir. Yanıtta satır "Yanıt giden kutusunda" + "N sn içinde gider · Kime" der; süre giden kutusunun geri alma süresidir ve durmaz; Geri al gönderimi iptal eder, metin kutuya döner. Gönderim yetişmişse "Yanıt çoktan gitti …" bilgisi Geri al'sız kalır. Odak karttaysa "Geri al"a geçer (onay satırında odak geri sayımı durdurmaz).
- **Hata.** Kartın içinde `alert` ikonu (`--danger`) + 12 `--ink-2` "Olmadı: …" (ne oldu + ne yapılabilir; ör. "Bu hesapta İstenmeyen klasörü yok. Postayı uygulamada aç ve bir klasöre taşı."), `role="alert"`. Hata yazan kart kendiliğinden kapanmaz; yanıtta metin kutuda kalır.
- **Geri sayım** posta halinde üzerine gelince ya da klavye odağı karttayken durur. Yanıt yazılan ya da geri alma bekleyen kart, yeni kartlar gelince "N posta daha" özetine düşmez.
- **Ayar.** Ayarlar › Bildirimler › Bildirim düğmeleri: beş anahtar (ikon, ad, kısa açıklama), satırın tamamı etiket; üçü seçiliyken diğerleri devre dışı ve altında "Üç düğme seçili. Başka birini seçmek için önce birini kapat." (`role="status"`). Masaüstünde altında kartın düğme sırasının önizlemesi ("Kartta"); mobilde 56 px satırlar, ayraç etiket hizasından.

## 6. Hareket

| Süre | Kullanım |
| --- | --- |
| 120 ms | anlık geri bildirim (üzerine gelme, basma) |
| 200 ms | durum değişimi (seçim, sekme) |
| 180–280 ms | menü, toast, pencere içeriği |
| 520 ms | **bildirim kartının gelişi** — uygulamanın tek "yazılmış anı" |

- Eğri: `cubic-bezier(.16, 1, .3, 1)`. Çıkış girişten hızlıdır. Zıplama/elastik eğri yok.
- Yalnız `transform`, `opacity`, `filter` ve gölge canlandırılır; genişlik/yükseklik/konum canlandırılmaz. Tek istisna: mobilde yüzen "Yeni posta" düğmesinin etiketi (200 ms; etiketin `max-width`'i, düğmenin `padding` ve `gap`'i); 56 px'lik tek öğe, yerleşimi etkilemez.
- `prefers-reduced-motion: reduce` iken kayma ve bulanıklık kalkar; renk ve saydamlık geçişi kalır.

## 7. İkonlar

- Kendi çizimimiz: 24 px ızgara, **1.6** çizgi, yuvarlak uç ve birleşim, dolgu yok (yıldız dolu hali hariç). Tek kaynak `app/src/lib/ui/icons.ts`, tek bileşen `Icon.svelte`.
- Başka kütüphane ikonu, emoji ya da unicode sembolü ikon yerine kullanılmaz.
- Boyutlar: 14 (satır içi işaret), 16 (küçük düğme, kenar çubuğu satırı), 18 (araç çubuğu, ikon rayı), 28–32 (boş durum).

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
| F5 | Postaları al (bağlantılar yenilenir, gelen kutuları eşitlenir) |
| Ctrl Enter | Gönder |
| Ctrl Z | Son işlemi geri al |
| Esc | Seçimi bırak / kapat / geri |
| V / L | Klasöre taşı / etiketle seçicisi |
| G sonra I / S / T / D / A / H / # | Gelen kutusu / Yıldızlılar / Gönderilenler / Taslaklar / Arşiv / Ertelenenler / Çöp |
| Shift O | Odak (tek tek işleme) kipi |
| ? | Kısayol sayfası |
| / | Arama (komut paleti) |
| Ctrl B | Kenar çubuğunu daralt / katman olarak aç |
| Ctrl + / Ctrl - / Ctrl 0 | Arayüz ölçeği büyüt / küçült / sıfırla |

Kısayollar metin girdisinde odak varken tek harfli tuşlarda çalışmaz (ölçek kısayolları çalışır).

İki tuşlu gezinme: G'den sonra sağ altta 12 px ipucu (`--raised`, `--shadow`) ikinci tuşları listeler; 1,5 sn içinde tanınmayan tuş ya da Esc zinciri bitirir ve tek harf eylemi tetiklemez. Sil için # ve Del ikisi de çalışır; arayüzde # yazılır.

Kısayol öğretimi: fareyle yapılan arşivle, ertele, yanıtla, sil, yıldızla ve okunmadı say eylemlerinden sonra toast kısayolu bir kez söyler (eylemin kendi toast'ına eklenir; toast'sız eylemde kısa ipucu toast'ı). Eylem başına en çok 3 kez; kısayol klavyeyle kullanıldıysa o eylem öğrenilmiş sayılır. Klavyeyle basılan düğme öğretmez. Ayarlar › Genel › "Kısayol ipuçları" kapatır (kişisel tercih).

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
- Dokunma/tıklama hedefi en az 28 px, birincil hedefler 34 px+. Görünüşü küçük kalması gereken denetimler hedefini görünmez kenarla (`::before`) büyütür: metin bağlantı düğmesi (`.link`, ≈ 29 px; mobilde ≥ 44), anahtar (56×44); arama çipinin kaldır düğmesi 28×28, renk seçici 28 px.
- Seçim, imleç, kaydırma çubuğu ve odak halkası paletten temalandırılır.

## 11. Platform

- Pencereler kendi başlık çubuğunu çizer (46 px ana, 40–42 px ikincil); küçült / büyüt / kapat sağda, kapatın üzerine gelince `#C42B1C`.
- Uygulama kapatılınca tepside kalır; tepsi ikonunda okunmamış sayısı.
- Linux Wayland'de bildirim kartı konumlanamıyorsa işlem düğmeli sistem bildirimi kullanılır.
- Arayüz ölçeği %90 / 100 / 110 / 120 / 135 / 150: Ctrl + / Ctrl - / Ctrl 0 ve Ayarlar › Genel (yalnız masaüstü). Tauri'de web görünümü yakınlaştırması (`setZoom`; CSS piksel oranı değişir, yazı ölçeğinin oranları korunur, dar pencere kipleri ölçekli genişliğe göre seçilir), tarayıcıda kök öğeye CSS `zoom`. Kişisel tercih, tarayıcı deposunda; mobil kabukta etkisiz.

## 12. Mobil uyarlama

Android sürümü aynı arayüz kodundan (`app/`) derlenir; kabuk `mobile` olunca mobil bileşenler yüklenir. Yukarıdaki jeton, yazı, renk, ikon ve hareket kuralları **aynen** geçerlidir; bu bölüm yalnız ölçü ve gezinmeyi uyarlar.

**Ölçüler**

| Öğe | Mobil |
| --- | --- |
| Dokunma hedefi | en az 44×44, birincil eylem 48 |
| Liste satırı | 72 px (sıkı görünüm 60), sistem yazı ölçeğiyle orantılı büyür; sol oluk 8 px: okunmamış noktası (8 px) + 40 px avatar, metin 78 px'ten başlar. Avatar ayrı düğmedir (52 px hedef): dokunmak çoklu seçimi başlatır. Birleşik görünümde (birden çok hesap, hesap seçili değil) saatin solunda 8 px, köşe 2 hesap rengi karesi (yuvarlak okunmamış noktasıyla karışmaz); renkli kenar çizgisi yok |
| Uygulama çubuğu | 56 px; listede `--surface` zemin, başlık içerikte (aşağıda) |
| Liste başlığı | **büyük başlık**: 24/600 görünüm adı + 12/500 `--ink-3` okunmamış sayısı, kaydırılan alanın başında; içerik akınca uygulama çubuğuna 19/600 kopyası belirir (120 ms saydamlık), alt çizgi o zaman çıkar. Çevrimdışıyken altında 12 `--ink-3` **durum satırı**: "Çevrimdışı · N gönderim bekliyor" (giden kutusunda zamanı gelmiş gönderimler) ya da "Çevrimdışı · değişiklikler bağlantı gelince eşitlenir"; bağlantı gelince 200 ms'de söner; engelleyen iletişim kutusu yok |
| Kategori çipleri | gelen kutusunda büyük başlığın altında, 32 px (alt sınır), köşe 999, `--line-strong` kenar; seçili `--accent-soft` zemin + `--accent-text`; adın yanında okunmamış satır sayısı 12/500 `--ink-3` (seçilide `--accent-text`; sayı yoksa yazılmaz, 99+); satır yatay kayar ve içerik akınca üste **yapışır** (`position: sticky`, kaydırıcının doğrudan çocuğu); dokunma alanı görünmez kenarla 44 |
| Okuma şeridi | **tek alt şerit**, 56 px: geniş "Yanıtla" (`--accent-soft` zemin + `--accent-text`, köşe 12, 44 px), Arşivle, Sil, "Diğer eylemler" (44 px ikon düğmeler). Ayrı hızlı yanıt kutusu yoktur; posta, uygulama çubuğu ve şerit dışındaki tüm alanı alır |
| Yanıt alanı | "Yanıtla" şeridi yerinde açar (280 ms, yalnız transform + opacity; azaltılmış harekette anlık): başta "Kime: …" (12 `--ink-2`), "Tam ekran yaz" ve kapat (44 px); altında hazır yanıt çipleri (kutu boşken), tek satırdan büyüyen kutu ve 40 px yuvarlak gönder. Zemin `--raised`, üstte `--line` |
| Başparmak bölgesi | sağ altta, güvenli alanın üstünde, 12 px arayla: **arama** (48 px yuvarlak, `--paper` + `--line`, gölgesiz) ve **yeni posta** |
| Yüzen "Yeni posta" düğmesi | **genişletilmiş**: 56 px yükseklik, kalem ikonu + "Yeni posta" etiketi, köşe 16, `--accent` + `--shadow`; ekranda tek birincil. Liste aşağı kaydırılınca etiket 200 ms'de kapanır (56 × 56, yalnız ikon), yukarı kaydırılınca ya da başa varınca geri gelir; yön değişimi 12 px'ten sonra sayılır |
| Gezinme satırı | 48 px (çekmece, alt sayfa menüsü); köşe 10 |
| Bölüm başlığı | 12/600 `--ink-3`, üstte 10 px boşluk (`.m-section`) |
| İkon | masaüstü boyutlarına 22 eklenir (çubuk ve satır eylemleri) |
| Yazı | ölçek 12 / 15 / 19 / 24 / 30 değişmez; okumada konu **19** (masaüstünde 24); liste başlığı 24 |
| Alt sayfa satırı | 48 px, köşe 16 (`--r-palette`), `--raised` + `--shadow-lg` |
| Çekmece | sol, 300 px, `--ground`; başlıkta **hesap kimliği** (36 px renkli rozet, ad 15/600, adres 12 `--ink-2`; dokununca hesap ayarları), çok hesapta marka satırı |
| Boş durum | 56 px `--accent-soft` disk içinde 26 px `--accent-text` ikon, başlık 19/600, gövde `--ink-3` 32ch |
| Ayar listesi | 56 px satır (hesap 64), satırlar etiket hizasından başlayan `--line` çizgiyle ayrılır (tam genişlik değil), gruplar 20 px boşlukla |

**Gezinme**

- Alt sekme çubuğu yok. Sol çekmece kenar çubuğunun karşılığıdır; **yalnız menü düğmesiyle** açılır (kenardan kaydırma Android'in geri hareketine bırakılır), örtüye dokunma, Esc ya da sola kaydırma kapatır: başlıkta hesap kimliği (tek hesap) ya da marka (çok hesap); görünümler (gelen kutusu, elek, ertelenen, bekleyen, yıldızlı, taslak, gönderilen, arşiv, çöp; 15/500, etkin 600 `--paper`/`--raised`); "Klasörler" bölümü (tek hesapta bölüm adı, çok hesapta katlanır hesap başlıkları; klasör satırı 15/400, ikon `--ink-3`); etiketler; kayıtlı aramalar; altta eşitleme durumu, Şimdi eşitle, tema ve Ayarlar.
- Yığın: liste → okuma → yazma; ayarlar yığına itilir. Her itme `history.pushState`; geri tuşu önce açık çekmece / alt sayfa / menüyü, sonra okuma ekranının yanıt alanını kapatır (yazılan korunur, şerit "Yanıta devam et" der), sonra yığından çıkar, kökte uygulamayı kapatır. Yanıt alanının üstüne alt sayfa açılabilir (katman yığını); "Tam ekran yaz" alanın girdisini yazma ekranına devreder.
- Menüler mobilde alt sayfa (`Sheet`) olarak açılır. Toast aynı kalır, alt çubuğun üstünde durur.
- Kaydırma: yön başına **ayarlanır** (Ayarlar › Genel › Kaydırma ve okuma; seçenekler ikon + adla, değer satırında da ikonuyla): Arşivle (`--accent`), Sil (`--danger`), İstenmeyen (`--danger`; hesabın `junk` rollü kutusuna taşır, kutu yoksa satır geri yaylanır, `haptic('reject')` ve "ne oldu + ne yapılabilir" toast'ı), Klasöre taşı (`--ink-2`; satır yerine döner, klasör seçici alt sayfası açılır, seçilince taşınır), Yarına ertele (`--amber`), Okundu / okunmadı (`--accent`), Sabitle (`--ink-2` zemin + `--paper`), Yok (o yön kapalı, satır yalnız 24 px direnir). Varsayılan sola Arşivle, sağa Ertele. Eşiğe (%40) kadar zemin nötr: `--paper` + `--ink-3` ikon; eşik geçilince zemin eylemin jetonuyla dolar, metin `--accent-ink`, ikon 120 ms'de 1.15'e büyür ve bir kez titreşir (`haptic('tick')`, yalnız Android). Bırakınca satırı çıkaran eylemde satır kenardan çıkar, okundu, sabitle ve klasör seçicide yerine döner; her eylem geri alma toast'ıyla gelir. 200 ms dönüş, yalnız `transform`; `prefers-reduced-motion` iken anlık.
- Çoklu seçim: avatara dokunmak ya da uzun basış (450 ms, `haptic('confirm')`) başlatır; seçim çubuğunda görünür adlı "Tümünü seç". Aşağı çekme eşitler.
- Okumada tek şerit: Yanıtla · Arşivle · Sil · Diğer eylemler (başta Tümünü yanıtla ve İlet, sonra taşı, etiket, sabitle, ertele…). Yıldız ve "okunmadı say" uygulama çubuğunda.
- Arşivleyip silince (taşı, ertele de): ayara göre listeye dönülür (varsayılan), listede bir alttaki ya da bir üstteki posta aynı ekranda açılır ve okundu sayılır; komşu yoksa listeye dönülür. Geri alınca ekran geri gelen postaya döner.
- Arama tam ekrandır ve alanı **altta**, klavyenin hemen üstündedir (geri düğmesi alanın solunda); boşken son aramalar (yalnız bu cihazda, en çok 6, "Geçmişi sil") ve işleç ipuçları alttan dizilir. Komut paleti ve klavye kısayolları yoktur; bildirim kartı, hızlı okuma ve pencere düğmeleri yoktur (işletim sistemi bildirimi kullanılır).
- Yazma: Kime boş ve odaktayken son yazışılan 5 kişi çip olarak (40 px, köşe 999; önce gönderilenlerin alıcıları, sonra Kişiler'den göndericiler); Gönder'e uzun basış "Sonra gönder" alt sayfasını açar (`haptic('confirm')`).

**Teknik**

- `index.html` görünüm alanı: `width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content`; güvenli alan `env(safe-area-inset-*)`.
- Koyu tema aynı `data-theme`; mobil i18n anahtarları `mob.*` önekiyle.
- Geçişler yalnız `transform` / `opacity`, 200–280 ms; mobil kabuk tembel yüklenir, listeler sanallaştırılır, Geist önyüklenir.
- Ölçüler `app/src/styles/mobile.css` değişkenlerinde tek yerde tutulur: `--m-appbar` 56, `--m-bar` 56, `--m-row` 72 / `--m-row-compact` 60, `--m-touch` 44 / `--m-touch-lg` 48, `--m-nav-row` 48, `--m-gutter` 16, `--m-chip` 32, `--m-drawer` 300, `--m-fab` 56 (köşe 16), `--m-line` 1.3334 (15 px'te 20 px satır aralığı, birimsiz); güvenli alan `--sat/--sab/--sal/--sar`. Paylaşılan sınıflar: `.m-chip` (+ `.m-chip-n` sayı), `.m-section` (bölüm başlığı), `.m-empty` (boş durum), `.thumb` (başparmak bölgesi), `.m-fab` (+ `.m-fab-label`, `.small`), `.m-search-fab`. İkon boyutu çubuklarda 22, yüzen düğmede 22.
- **Sistem yazı ölçeği:** Android WebView kullanıcının yazı büyütmesini px yazılara da uygular. Metin taşıyan öğelerde yükseklik alt sınırdır (`min-height`; satırlar, çubuklar, çipler, menü ve çekmece satırları, düğmeler), satır aralığı birimsizdir (`--m-line`). Sanal listenin satır ve grup yükseklikleri görünmez bir ölçü metninden okunan oranla büyür (1.3× ölçekte satır 94 px). `setTextZoom(100)` ile ölçek kapatılmaz.
- **Titreşim:** yalnız Android kabuğunun `haptic` komutu (`tick` eşik, `confirm` uzun basış, `reject` ret); masaüstünde ve tarayıcıda komut yoktur, çağrı sessizce geçer.
- Liste sanal yerleşimi (`lib/ui/virtual.ts`) büyük başlık bloğunun yüksekliği kadar ötelenir (`scrollTop - headH`); başlık ve çipler kaydırılan alanın içindedir, satır konumları değişmez. Görünüm değişince başa dönülür; yerleşim ölçüleri değişince (yeni posta, saat) kaydırma konumu korunur.
- Seçim kipinde uygulama çubuğu `--accent` zemin + `--accent-ink`. Örtü `color-mix(in srgb, var(--ink) 42%, transparent)`. Hareket: ekran itme 280 ms (`translateX(28px)` + opacity), çekmece 280 ms, kaydırma dönüşü ve alt çubuk 200 ms; hepsi `cubic-bezier(.16,1,.3,1)`.
- Kenardan kaydırma çekmeceyi **açmaz** (Android'in hareketle gezinmesinde sol kenar geri hareketidir); uzun basış 450 ms. Toast mobilde yüzen düğmenin ve şeridin üstünde durur.
