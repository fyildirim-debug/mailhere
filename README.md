# Mailhere

Windows ve Linux için hızlı, sakin, klavyeyle yönetilen masaüstü e-posta istemcisi.

Yeni posta geldiğinde koca bir pencere açılmaz: ekranın köşesinde küçük bir kart belirir. "Aç" dediğinde yalnız o postayı gösteren hızlı okuma penceresi gelir; yanıtlarsın, arşivlersin, işine dönersin.

![Mailhere ana pencere](docs/img/ana-pencere.png)

| Köşe kartı | Hızlı okuma |
| --- | --- |
| ![Yeni posta bildirimi](docs/img/bildirim-karti.png) | ![Hızlı okuma penceresi](docs/img/hizli-okuma.png) |

![Koyu tema](docs/img/koyu-tema.png)

## Öne çıkanlar

- **Köşe kartı ve hızlı okuma** — bildirimler yalnız kişilerden gelen postada çıkar; bülten ve otomatik bildirimler tepsideki sayaçta kalır. Sessiz saatler, bir saat sessize alma, kart konumu ve süresi ayarlanır.
- **Akıllı süzgeç** — gelen kutusu Kişiler, Bildirimler ve Bültenler diye kendiliğinden ayrılır.
- **Klavye önce** — `J/K` gezin, `E` arşivle, `H` ertele, `R` yanıtla, `Ctrl K` her şeyi bul.
- **Geri alınabilir her şey** — arşivleme, silme, erteleme ve gönderme birkaç saniye geri alınabilir.
- **Her hesap** — IMAP/SMTP (ayarlar kendiliğinden bulunur), Gmail ve Outlook / Microsoft 365 için tarayıcıyla giriş (OAuth 2, XOAUTH2), Stalwart gibi JMAP sunucuları (anlık bildirim dahil).
- **Klasörler** — hesap başına klasör ağacı (alt klasörler, okunmamış sayısı); istemediğin klasörü gizlersin, gizli klasör eşitlenmez. Gönderilenler, Taslaklar, Arşiv, Çöp ve İstenmeyen klasörleri elle seçilebilir.
- **Kurallar** — gelen postayı göndereninden, konusundan, metninden ya da türünden tanıyıp klasöre taşır, arşivler, siler, okundu sayar, yıldızlar, bildirimsiz bırakır ya da başka bir adrese iletir (döngü korumalı). Göndericiyi tek tıkla engellersin.
- **Planlama** — "sonra gönder" ile zamanlanan postalar Planlananlar'da durur; hemen gönderilir, düzenlenir ya da iptal edilir.
- **Yazarken** — hesap başına birden çok imza (yeni posta ve yanıt için ayrı), şablonlar, kendi hızlı yanıtların; taslak kendiliğinden sunucuya kaydedilir, iletilen postanın ekleri de gider.
- **Okurken** — konuşma görünümü (bir konuşmanın tüm iletileri alt alta), kişi kartı (son konuşmalar, önemli kişi, engelle, kural), takvim daveti kartı (Katılıyorum / Belki / Katılmıyorum, takvime ekle), yazdırma ve `.eml` olarak kaydetme.
- **Yanıt hatırlatması ve elek** — "Yanıt gelmezse hatırlat" ile gönderdiğin posta, yanıt gelmezse seçtiğin gün "Yanıt bekleniyor" işaretiyle gelen kutusunun başına döner; yanıt gelince hatırlatma kendiliğinden kapanır. İsteğe bağlı yeni gönderici eleği ilk kez yazanları gelen kutusu yerine Elek'te bekletir. Şablon ve hızlı yanıtlarda `{ad}`, `{benim_adim}`, `{tarih}` gibi değişkenler eklenirken doldurulur.
- **Arama ve süzgeç** — `from:`, `to:`, `konu:`, `has:ek`, `is:okunmamış`, `in:klasör`, `tag:etiket`, `önce:` / `sonra:` işleçleri; listede okunmamış, yıldızlı, ekli süzgeci. Sık kullandığın aramayı "Aramayı kaydet" ile kenar çubuğuna alırsın.
- **Çoklu seçim** — Ctrl + tık, Shift + tık, Ctrl A ya da Shift J/K ile birden çok posta seç; toplu işlem paneli arşivler, siler, okundu sayar, yıldızlar, erteler, klasöre taşır, etiketler. Okuma panelinden bir göndericinin tüm postalarını tek seferde arşivler, çöpe atar ya da yalnız en yenisini bırakırsın; hepsi onaylı ve geri alınabilir.
- **Sabitleme ve etiketler** — `P` ile sabitlenen posta gelen kutusunun ve klasörün en üstünde "Sabitlenenler" grubunda durur; renkli yerel etiketler kenar çubuğundan, kurallardan ve `tag:` işlecinden erişilir. İkisi de yalnız bu bilgisayarda tutulur, sunucuya yazılmaz.
- **Bültenler** — abonelikten tek tıkla çıkma (RFC 8058, yalnız genel HTTPS adresine) ya da posta ile.
- **Otomatik yanıt** — JMAP sunucularında (ör. Stalwart) sunucu tarafında tatil yanıtı; uygulama kapalıyken de çalışır.
- **Ayrıntılı hesap ayarları** — gönderen adı, renk, hesap başına bildirim, sunucu ve parola değişikliği (önce bağlantı denenir).
- **Çevrimdışı güvenli** — arşivleme, bayrak, taşıma gibi sunucu işlemleri kalıcı bir kuyrukta bekler, bağlantı gelince sırayla uygulanır; uygulama kapansa da kaybolmaz.
- **Gizlilik varsayılan** — parolalar ve jetonlar işletim sisteminin anahtar zincirinde; yerel posta veritabanı diskte şifreli (SQLCipher, AES-256; anahtarı anahtar zincirinde); uzak görseller ve izleme pikselleri kapalı; HTML posta betiksiz ve yalıtılmış gösterilir. Koyu temada kendi zemini olmayan postalar da koyu zeminde açılır. Kendinden imzalı sertifika ancak parmak izini görüp onayladığında kabul edilir.
- **Sakin bildirim** — tam ekran uygulama ya da sunum açıkken kart çıkmaz.
- **İmzalı güncelleme** — yeni sürüm uygulama içinden kurulur; imzası tutmayan paket kurulmaz.
- **Hafif** — Tauri 2 ile işletim sisteminin kendi web görünümü kullanılır; ayrı bir tarayıcı gömülmez.

Güvenlik modeli, alınan önlemler ve açık kalanlar: [`docs/GUVENLIK.md`](docs/GUVENLIK.md).

## Geliştirme

Gerekenler: Node 22, pnpm 10, Rust (stable), Perl ve `make` (SQLCipher'in OpenSSL'i kaynaktan derlenir; ilk derleme birkaç dakika sürer). Linux'ta ayrıca `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev`. Windows'ta Visual Studio derleme araçları ve [Strawberry Perl](https://strawberryperl.com) gerekir (Git Bash'in perl'i olmaz; gerekirse `OPENSSL_SRC_PERL` ile yolu verilir).

```bash
pnpm install
pnpm dev            # arayüz tarayıcıda, örnek verili sahte arka uçla
pnpm tauri dev      # masaüstü uygulaması
```

Örnek veriyle masaüstü uygulaması (gerçek hesap gerekmez):

```bash
MAILHERE_DEMO=1 MAILHERE_SECRETS=memory MAILHERE_DATA_DIR=$(mktemp -d) pnpm tauri dev
```

İlk üç değişken yalnız geliştirme derlemesinde (`pnpm tauri dev`) geçerlidir; sürüm derlemesi bunları yok sayar.

| Ortam değişkeni | Etkisi |
| --- | --- |
| `MAILHERE_DEMO=1` | hesap yoksa örnek hesapları ve postaları yükler (bellek kipini de açar) |
| `MAILHERE_DEMO_NOTIFY=<sn>` | açılıştan bu kadar sonra örnek bildirim kartı gösterir |
| `MAILHERE_SECRETS=memory` | parolaları ve veritabanı anahtarını anahtar zinciri yerine bellekte tutar; `MAILHERE_DATA_DIR` verilmezse veriler her açılışta yeni bir geçici klasöre yazılır |
| `MAILHERE_DATA_DIR=<klasör>` | veritabanı ve önbellek klasörü (bellek kipinde boş olmalı) |
| `MAILHERE_GOOGLE_CLIENT_ID`, `MAILHERE_GOOGLE_CLIENT_SECRET` | Google ile giriş (Gmail, XOAUTH2); masaüstü uygulaması OAuth istemcisi |
| `MAILHERE_MICROSOFT_CLIENT_ID` | Microsoft ile giriş (Outlook / Microsoft 365, XOAUTH2); genel istemci |

OAuth istemci kimlikleri koda yazılmaz: derlemede (`pnpm tauri build`) bu ortam değişkenlerinden gömülür; aynı adlı değişkenler çalışırken yalnız geliştirme derlemesinde (`pnpm tauri dev`) bunları ezer. Boş ya da tanımsızsa o sağlayıcının düğmesi uygulama parolasıyla IMAP yoluna yönlendirir. CI'da depo gizli değerlerinden (`secrets`) gelir. Dönüş adresi `http://127.0.0.1:<rastgele kapı>/`: Google'da istemci türü "Masaüstü uygulaması" seçilir (ek kayıt gerekmez); Microsoft'ta "Mobil ve masaüstü uygulamaları" platformuna `http://127.0.0.1` dönüş adresi eklenir (kapı eşleşmede yok sayılır).

Denetimler:

```bash
pnpm check && pnpm test && pnpm e2e
cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Gerçek bir IMAP sunucusuna karşı tümleşik test (kutuları boşaltır, yalnız test sunucusunda çalıştır):

```bash
MH_TEST_IMAP=127.0.0.1:10143:kullanici:parola:none cargo test -p mh-sync --test imap --test drafts_imap
```

XOAUTH2 girişi için Dovecot `oauth2` passdb'sine karşı test (sahte sağlayıcıyı test kendisi `127.0.0.1:<introspection kapısı>` üzerinde açar; Dovecot ayarında `introspection_url = http://127.0.0.1:<kapı>/introspect`, `introspection_mode = post`, `username_attribute = username`):

```bash
MH_TEST_XOAUTH2_IMAP=127.0.0.1:10443:kullanici:18443 cargo test -p mh-sync --test xoauth2
```

Paketler (`target/release/bundle/`): Linux'ta `.deb`, `.rpm`, `.AppImage`; Windows'ta NSIS `.exe` ve `.msi`.

```bash
pnpm tauri build
```

## Yapı

| Klasör | İçerik |
| --- | --- |
| `app/` | Svelte 5 + TypeScript arayüzü; tüm pencereler tek uygulamada (`#/main`, `#/notify`, `#/quick/:id`, `#/compose/:mod/:id`) |
| `src-tauri/` | Tauri kabuğu: pencereler, tepsi, komutlar |
| `crates/mh-core` | arayüzle ortak tipler (sözleşme) |
| `crates/mh-store` | SQLite + FTS5 yerel depo (SQLCipher ile şifreli) |
| `crates/mh-mime` | ayrıştırma, HTML temizleme, akıllı kategori |
| `crates/mh-sync` | sunucu ayarı keşfi, IMAP eşitleme, anlık bildirim (IDLE) |
| `crates/mh-send` | SMTP gönderim, giden kutusu, geri alma |
| `crates/mh-notify` | bildirim kuralları |

Program kuralları [`CLAUDE.md`](CLAUDE.md), tasarım dili [`docs/TASARIM.md`](docs/TASARIM.md), yol haritası [`docs/PLAN.md`](docs/PLAN.md).

## Lisans

MIT © Furkan YILDIRIM. Temiz oda geliştirmesidir: başka bir e-posta istemcisinden kod, tasarım ya da varlık alınmamıştır. Yazı tipi Geist (SIL OFL 1.1).
