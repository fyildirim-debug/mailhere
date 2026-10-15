# Mailhere — güvenlik modeli

Son denetim: 2026-10-13 (beş alanda kod denetimi; bulgular doğrulanıp düzeltildi). Bu belge neyin korunduğunu, nasıl korunduğunu ve açık kalanları özetler.

## Kimden korunuyoruz

| Saldırgan | Hedefi |
| --- | --- |
| Posta gönderen herkes | uygulamada kod çalıştırmak, dosya okumak, okundu bilgisi sızdırmak, oltalama, uygulamayı kilitlemek |
| Ağdaki saldırgan / sahte DNS | parola ya da jeton çalmak, bağlantıyı şifresize düşürmek |
| Kötü niyetli posta ya da JMAP sunucusu | belleği doldurmak, parolayı başka adrese yönlendirmek |
| Aynı bilgisayardaki başka kullanıcı ya da süreç | postaları ve ekleri okumak |
| Sahte güncelleme / tedarik zinciri | imzasız ya da eski sürüm kurdurmak, imza anahtarını çalmak |

## Önlemler

**Gizli bilgiler ve diskteki veri**
- Parola ve OAuth yenileme jetonu yalnız işletim sisteminin anahtar zincirinde (`Mailhere` hizmeti). Erişim jetonu yalnız bellekte.
- Yerel veritabanı SQLCipher ile şifreli (AES-256, sayfa başına HMAC). 256 bitlik anahtar ilk açılışta üretilir, yalnız anahtar zincirinde durur (`db-key`). Anahtar zinciri yoksa şifresiz veritabanına düşülmez.
- Eski şifresiz veritabanı ilk açılışta, yarıda kesilmeye dayanıklı biçimde şifreliye çevrilir.
- Veri klasörü 0700, veritabanı ve açılan ekler 0600. `secure_delete` ve FTS güvenli silme açık. Açılan ekler bir günde ve hesap kaldırılınca silinir.
- Windows'ta veritabanı dolaşan profilde değil, yerel klasörde (`LocalAppData`).
- Parola, jeton ve anahtar günlüğe ve hata mesajına yazılmaz. IMAP/SMTP/HTTP kitaplıklarının ayrıntılı günlüğü kapalı.

**Posta içeriği**
- HTML Rust'ta temizlenir (ammonia + CSS izin listesi). Betiksiz, CSP'li, Referer göndermeyen bir çerçevede gösterilir. Uzak içerik varsayılan kapalı.
- Aşırı iç içe ya da büyük HTML doğrusal bir ön taramayla yakalanır, düz metne düşer. Ayrıştırma ayrı iş parçacığında yapılır.
- Görünen ad, konu ve ek adlarından satır sonu, denetim ve yön değiştiren karakterler atılır. Adres gibi görünen ad gerçek adresi gizleyemez.
- Yanıtın gideceği tüm adresler görünür. Gönderenin alan adı dışına giden yanıtta uyarı çıkar.
- Koyu temada postanın renkleri (zemin, yazı, kenar) ana pencereden satır içi stil olarak uyarlanır; çerçevede yine betik çalışmaz, içerik eklenmez, ağ istenmez. En çok 6000 öğe incelenir.
- İzinli uzak görsel çerçevede yüklenemezse (ör. sunucu `Cross-Origin-Resource-Policy: same-origin` verdiği için) `remote_images` komutu görseli Rust'ta indirir ve `data:` adresi olarak verir. Yalnız göndericisi izin listesindeki postanın temizlenmiş HTML'inde gerçekten geçen http(s) adresler indirilir; çerez, Referer, kimlik bilgisi ve vekil sunucu yok; sunucu adı önce çözülür, bağlantı yalnız denetlenen adreslere kurulur, bu bilgisayar (loopback) ve link-local hedefler reddedilir (özel ağ serbest: şirket içi bültenlerin görselleri oradadır, WebView de onları yükleyebilir); yönlendirme en çok 3 adım, yalnız http(s)'e ve her adım yeniden denetlenir; yanıt `image/*` olmalı (SVG dahil; `img` bağlamında betik çalışmaz); görsel başına 8 MB, çağrı başına 32 MB ve 40 adres; görsel başına 15 sn, çağrı başına 45 sn; aynı anda 4 indirme. Günlüğe adres değil yalnız sunucu adı yazılır. Komut ana pencere ve hızlı okuma penceresine açıktır.
- Ekler yalnız izin listesindeki türlerde doğrudan açılır, diğerleri klasörde gösterilir. Windows'ta "İnternet'ten geldi" işareti konur. Dosya adı kısaltılırken uzantı korunur.
- İzleme pikselleri (1×1 ya da sıfır boyutlu, gizli, ya da kendi kısa listemizdeki izleme sunucularından/açılma yollarından gelen görseller) temizlemede tümden atılır ve sayılır; göndericiye izin verilmiş olsa da yüklenmez, `remote_images` ile de indirilemez. Bağlantılardan bilinen izleme parametreleri (`utm_*`, `fbclid`, `gclid`, `mc_eid`, `_hsenc` …) sökülür; yönlendirme sarmalayıcıları açılmaz, adres yeniden kodlanmaz. Sayılar okuma alanında gösterilir (`trackers`, `cleanedLinks`). Her iki geçiş de doğrusaldır.
- Metni bir adres ya da alan adı gibi görünen ama başka bir siteye giden bağlantıya (kullanıcı bilgisi hilesi `https://banka.com@kotu.example` dahil) Rust gerçek sunucu adını `data-mh-host` olarak yazar; postanın kendi yazdığı bu öznitelik ammonia'da atılır. Arayüz böyle bir bağlantıyı açmadan önce gerçek hedefi gösteren bir onay ister; varsayılan eylem vazgeçmektir.
- Gönderen uyarısı (`senderWarning`): alıcı sunucunun eklediği en üstteki `Authentication-Results` başlığında DMARC başarısızsa ya da DKIM ve SPF birlikte başarısızsa; görünen adda başka bir siteye ait e-posta adresi varsa; gönderen alan adı (punycode çözüldükten sonra) bir etikette Latin ile Kiril/Yunan harfleri karıştırıyorsa ya da Latin üst alan altında yalnız Latin'e benzeyen Kiril/Yunan harflerden oluşuyorsa (`аррӏе.com`); Türkçe ve başka Latin harfli uluslararası alan adları (`şirket.com.tr`) uyarı almaz, çözülemeyen punycode şüpheli sayılır. Gönderilmiş ve taslak postada uyarı üretilmez. Sahte bir gönderici kendi `Authentication-Results` başlığını alta ekleyebilir; yalnız en üstteki okunur, alıcı sunucu hiç eklemiyorsa bu işaret çıkmaz.
- Doğrulama kodu (`otp`) yalnız yerelde, konu ve düz metinden kurallarla bulunur; ağa ya da günlüğe gitmez. Masaüstü bildirim kartı kodu kopyalama düğmesi için alır; Android bildirimi henüz kullanmaz.
- Abonelikten çıkma yalnız kullanıcı isteyince yapılır. Tek tık isteği (RFC 8058) yalnız genel internetteki bir HTTPS adresine gider: sunucu adı önce çözülür, adreslerden biri bile bu bilgisayarı ya da yerel ağı (loopback, özel, link-local, ULA, CGNAT, çoklu yayın, belirtilmemiş) gösteriyorsa istek gönderilmez; bağlantı yalnız denetlenen adreslere, vekil sunucusuz kurulur. Çerez, Referer ve kimlik bilgisi gönderilmez, yönlendirme izlenmez, 10 sn zaman aşımı. `mailto:` ile çıkmada posta tek adrese, postayı alan hesaptan gider; başlık `List-Unsubscribe` değerleri sınırlı ve denetimli ayrıştırılır.

**Uygulama kabuğu**
- Her pencere yalnız kullandığı komutları çağırabilir (`src-tauri/capabilities/*.json`, `build.rs`). Bildirim kartı ve hızlı okuma ayar, hesap ve sertifika komutlarına erişemez. Hızlı okuma ayarlardan yalnız hızlı yanıtları yazabilir (`quick_replies_set`; temizlenir, en çok 8 × 120 karakter).
- Postaya yalnız kullanıcının işletim sisteminin dosya penceresinde seçtiği dosyalar eklenebilir. Arayüz kendi yol yazamaz.
- Pencereler uygulama dışı adrese yönlendirilemez. Dış bağlantılar yalnız `http`, `https` ve `mailto` ile, varsayılan tarayıcıda açılır.
- Deneme anahtarları (`MAILHERE_DEMO`, `MAILHERE_SECRETS=memory`) ve OAuth kimliğini çalışırken değiştirme yalnız geliştirme derlemesinde çalışır.

**Kurallar ve arama**
- Kurallar (ve kuralla iletme) yalnız ana pencerenin komutlarıyla kurulur; bildirim kartı ve hızlı okuma bu komutlara erişemez.
- Kuralla iletilen postaya `Auto-Submitted: auto-forwarded` eklenir. Döngü koruması: hesapların kendi adreslerinden gelen ve `Auto-Submitted` başlığı taşıyan postalar iletilmez; aynı posta aynı kuralla bir kez iletilir; kural başına saatte en çok 30 iletme. Kayıt veritabanında tutulur, yeniden başlatınca sıfırlanmaz.
- İletme adresi tek ve geçerli bir adres olmalı (en çok 254 karakter, ad ya da liste kabul edilmez). Kural iletmesi gönderilemezse taslağa çevrilmez.
- Arama işleçlerinin (`from:`, `in:`, `önce:` …) değerleri SQL'e hep parametreyle gider; serbest metin FTS sorgusuna yalnız tırnaklı önek terimleri olarak girer.

**Okuma: konuşma, kişi kartı, yazdırma, takvim daveti**
- Yazdırma uygulama arayüzünden ayrı, kum havuzlu (`allow-same-origin allow-modals`) ve betiksiz gizli bir çerçevede yapılır; yalnız o çerçeve basılır. Gövde Rust'ta temizlenmiş HTML'dir, başlıklar kaçışlanır; çerçevenin CSP'si uzak görseli yalnız göndericiye izin verildiyse yükler, Referer gitmez.
- `.eml` kaydetme yalnız işletim sisteminin kaydet penceresinde seçilen yola yazar (arayüz yol veremez); dosya 0600, uzantı `.eml` zorlanır, önerilen ad konudan temizlenir (`sanitize_filename`).
- Takvim daveti (`text/calendar`) Rust'ta ayrıştırılır: parça en çok 512 KB, 20 bin satır, satır 16 KB, metin alanları 500 karakter, katılımcı sayısı sınırlı; bozuk girdi panik üretmez. Yalnız METHOD REQUEST/CANCEL kart olur. Zaman dilimi postadaki VTIMEZONE'dan hesaplanır; bilinmeyen dilim yerel saat sayılır.
- Davete yanıt yalnız kullanıcı düğmeye basınca, postayı alan hesaptan, davetin ORGANIZER satırındaki tek `mailto:` adresine gider (adres kartta görünür); METHOD REPLY, aynı UID/SEQUENCE, PARTSTAT. İptal edilmiş davete yanıt gönderilmez. Not: düzenleyen adresi gönderenin elindedir; yanıt o adrese postanın okunduğunu ve kullanıcının adresini bildirir.
- "Takvime ekle" takvim parçasını açılan ekler klasörüne (0700/0600) yazar ve varsayılan uygulamayla açar; `.ics` izin listesindedir.
- Kişi kartı sorguları (gönderen/alıcı adresi) SQL'e parametreyle gider; bozuk adres (`is_full_address`) reddedilir. Konuşma görünümü aynı hesapla sınırlıdır; en çok 100 ileti.

**Ağ ve kimlik doğrulama**
- TLS zorunlu; şifresiz bağlantı yalnız `localhost`'a. STARTTLS reddedilirse düz metne düşülmez.
- Geçersiz sertifika yalnız sunucunun gerçekten sunduğu sertifika için onaylanabilir. Onay ayrıca işletim sisteminin kendi penceresinde istenir. Büyük sağlayıcılarda (Gmail, Outlook…) onay verilemez.
- JMAP parolası yalnız oturumun kendi kökenine gider. Başka kökene, `http`'ye ya da bu makineye yönlendirmede gönderilmez. Sunucu yanıtları boyutla sınırlıdır.
- Hesabın sunucu adresi, kapısı ya da kullanıcı adı değişince kayıtlı parola yeni sunucuya gönderilmez, yeniden istenir. Sunucu ya da parola değişikliği kaydedilmeden önce bağlantı ve giriş denenir (sertifika kuralı aynı).
- DNS (SRV) ile bulunan sunucu e-posta alan adı dışındaysa hesap eklerken uyarı çıkar. Otomatik yapılandırma yalnız HTTPS'ten okunur.
- OAuth: PKCE (S256), rastgele `state`, yalnız `127.0.0.1`'de tek kullanımlık dönüş adresi.
- SMTP'de makine adı yerine `EHLO [127.0.0.1]` gönderilir.

**Güncelleme ve yayın**
- Güncellemeler minisign ile imzalı. İmzadaki sürüm duyurulan sürümle eşleşmeli (`requireSignedVersion`), eski sürüme düşürülemez.
- İmza anahtarı paket derlemesine hiç girmez. Paketler önbelleksiz, imzasız derlenir; ayrı küçük bir işte, `yayin` ortamında imzalanır. Yazma izni yalnız sürüm taslağı işinde.
- Bağımlılıklar `--locked`. `cargo audit`: bilinen açık yok (yalnız Tauri/GTK üzerinden gelen iki bakım uyarısı: glib, proc-macro-error). `pnpm audit`: bilinen açık yok. Dependabot ayda bir güncelleme önerir.

## Android

Taslak. Android kabuğu ayrı depoda (`mailheremobile`) derlenir; ortak çekirdek (`crates/*`, `crates/mh-app`) aynıdır. Henüz uygulanmamış maddeler **(planlanan)** diye işaretlidir; uygulandıkça işaret kalkar.

- Çekirdek ayrımı: masaüstüne özgü parçalar (anahtar zinciri, tepsi, bildirim kartı penceresi, güncelleyici, tek örnek) kabuk crate'indedir; `crates/mh-app` bunları bağlamaz. Platforma göre değişen işler `Platform` trait'inden geçer, ortak kod pencere ya da dosya sistemine doğrudan dokunmaz.
- Komut listesi tek kaynaktan gelir (`shared_commands!`, `SHARED_COMMANDS`, `commands.txt`); mobil kabuk masaüstü komutlarını (`open_quick`, `notify_*`, `demo_notify`, `update_*`) bağlamaz. Her kabuğun `capabilities/*.json` dosyası yalnız kendi komutlarına izin verir.
- Pencere yalnız uygulamanın kendi adreslerinde kalır; geliştirme sunucusu adresine yalnız geliştirme derlemesinde ve yapılandırmadaki `devUrl` ana makinesi için izin verilir.
- Veri yolları uygulamanın özel klasörleri (`app_local_data_dir`, `app_cache_dir`); veritabanı yine SQLCipher ile şifreli, dosya izinleri aynı (0700 / 0600).
- **(planlanan)** Parola, OAuth yenileme jetonu ve veritabanı anahtarı `AndroidKeyStore` ile korunan kasada: donanım destekli AES-256-GCM anahtarı, 12 baytlık rastgele IV, anahtar adı AAD; şifreli değerler `mailhere-kasa.xml` tercih dosyasında. Kasaya yalnız Rust tarafı (`SecretStore` köprüsü) erişir.
- **(planlanan)** `android:allowBackup="false"` ve `dataExtractionRules` ile tüm uygulama verisi (veritabanı, kasa, önbellek) bulut yedeklemesi ve cihazlar arası aktarımın dışında tutulur.
- **(planlanan)** Ekler: seçilen dosyalar (`content://`) uygulamanın önbelleğine kopyalanır ve yalnız o kopyalar postaya eklenir; açma `FileProvider` ile yalnız o dosya için verilen geçici okuma iznidir; kaydetme sistemin dosya seçicisi (SAF) ile kullanıcının seçtiği konuma yazılır. Uygulama başka uygulamaların dosyalarını yol vererek okuyamaz.
- **(planlanan)** Bildirim: Android 13+ izin istenir; kanal `yeni-posta`; bildirim metni gönderen ve konudan ibarettir, gövde ve ek adı taşınmaz.
- **(planlanan)** OAuth dönüşü cihazda `127.0.0.1` döngü adresi üzerinden; tarayıcı özel sekmesi kapanınca dinleyici kapanır. Cihazda doğrulanacak.
- **(planlanan)** QR tarayıcı kendi etkinliğimiz (CameraX + zxing core, Apache-2.0); ML Kit ya da başka bulut hizmeti yok. Kamera izni yalnız tarama ekranında istenir.
- **(planlanan)** Ağ: TLS kökleri `webpki-roots` (sistem deposu değil); sertifika onayı masaüstüyle aynı akış. Ağ değişince eşitleme yeniden denenir.
- **(planlanan)** Sürüm imzası: APK/AAB CI'da `apksigner` ile doğrulanır; imza anahtarı yalnız sürüm işinin gizli değerlerindedir.

## Cihazlar arası aktarım

Masaüstündeki hesaplar, ayarlar, kurallar ve etiketler telefona QR koduyla, yalnız yerel ağ üzerinden aktarılır. Postalar, taslaklar, giden kutusu ve cihaza özgü ayarlar aktarılmaz. Kripto ve ağ katmanı `crates/mh-transfer`'da, paketin kurulması/uygulanması `crates/mh-app/src/transfer.rs`'de, arayüz `app/src/windows/main/settings/DevicesSection.svelte` (bilgisayar) ve `app/src/windows/mobile/TransferReceive.svelte` (telefon) içindedir.

- Kod (`mailhere://aktar?v=1&h=<adresler>&p=<kapı>&t=<16 B>&k=<32 B>`) yalnız QR'da ve ekrandaki metinde bulunur; **K anahtarı ağdan hiç geçmez**. Kodda yalnız özel IPv4, CGNAT (100.64/10; Tailscale gibi örtü ağları) ve IPv6 ULA adresleri duyurulur (en çok 6); bağlanırken arayüz adı gerektiren link-local adresler ve genel adresler asla yazılmaz, genel adres içeren kod telefonda reddedilir. Bilinmeyen sürüm (`v>1`) "uygulamayı güncelle" hatası verir.
- Anahtar türetme HKDF-SHA256 (tuz T, IKM K) ile üç ayrı anahtar: şifreleme, kimlik, onay. Telefon kimliğini 68 baytlık selamla (HMAC-SHA256, rastgele nonce) kanıtlar; paket AES-256-GCM ile gider (aad = T + istemci nonce'u); uygulandı onayı ayrı anahtarla şifreli ve doğrulanmış. Yanıt üst sınırı 8 MiB; aşan uzunluk gövde okunmadan reddedilir.
- Sunucu yalnız yerel ağ eşlerini kabul eder (özel IPv4, CGNAT, link-local, ULA; loopback ve genel asla). Paket tek bir istemciye **bir kez** verilir; hatalı selam yanıtsız kapanır (zamanlama ya da varlık sızdırmaz) ve sayılır, 5 hatalı denemede teklif kapanır; hiç bayt göndermeden kapanan bağlantı (telefonun öteki adreslere açtığı yedek bağlantılar) sayılmaz. Teklif 180 s, onay 8 dk, okuma 10 s, yazma 30 s; aynı anda en çok 8 bağlantı, tek teklif. Telefon tarafı adres başına 3 s, toplam 30 s bekler.
- Gizli değerler (K, türetilmiş anahtarlar, düz paket, kod metni) bellekte `Zeroizing` ile tutulur, diske yazılmaz; `Debug` çıktıları ve hata mesajları anahtar içermez.
- Paket uygulanırken mevcut doğrulayıcılar çalışır (ayar sınırları, hesap/kural/etiket üst sınırları, ad çakışmasında etiket eşleme). Parolalar varsayılan dahildir ve onay kutusuyla kapatılabilir.
- Ekranda uyarılar: aynı Wi-Fi, güvenlik duvarı, ekran paylaşımı sırasında QR göstermeme. Kod metni varsayılan gizlidir; gösterilince "yalnız telefondaki Mailhere'ye yapıştır" uyarısı eşlik eder. Bölümden çıkınca açık teklif iptal edilir. QR `{@html}` ile değil, modül başına `rect` ile çizilir; kod metni günlüğe ya da toast'a yazılmaz. Tüm paketi QR'a koymak (ağsız aktarım) reddedildi; ağsız seçenek sonraki sürümde parola korumalı şifreli dosya olarak planlanıyor.
- Açık kalan: aynı yerel ağdaki bir saldırgan hatalı denemelerle teklifi kapatabilir (yalnız erişilebilirlik; yeni kod oluşturulur). QR ekranda göründüğü sürece fotoğraf ya da ekran paylaşımı K'yi ifşa eder. Sanal ağ (VPN/konteyner) adresleri kodda sonda duyurulabilir; K olmadan işe yaramaz.

## Açık kalanlar

- **GitHub ayarı:** `yayin` ortamına onaylayıcı eklenmeli (Settings → Environments). İlk sürümde ortam kendiliğinden oluşur.
- **CI eylemleri** sürüm etiketine bağlı, commit özetine (SHA) değil. Dependabot günceller; daha sıkı isteniyorsa SHA'ya sabitlenebilir.
- **Uzak görsel izni** gönderen adresine göre veriliyor. Gönderen adresi sahtelenebilir, bu yüzden izinli bir gönderici taklit edilirse uzak görseller yüklenebilir (tanınan izleme pikselleri yine yüklenmez; tanınmayan bir izleyici yüklenebilir). Kimlik doğrulama başarısızsa okuma alanında uyarı çıkar ama izin kendiliğinden geri alınmaz.
- **Karışık alfabeli alan adları** yalnız gönderen adresinde işaretlenir; bağlantı hedeflerinde ayrıca işaretlenmez (metni farklı bir siteyi gösteren bağlantı onay ister).
- **Aldatıcı bağlantı** tanıma, bağlantı metninin adres gibi göründüğü durumla sınırlı; "Hesabına gir" gibi düz metinli bağlantıda hedef ayrıca gösterilmez. Kayıtlı alan adı ayrımı kamu sonek listesi yerine kısa bir ikinci düzey ad listesiyle yaklaşık yapılır.
- **Abonelikten çıkma başlığının DKIM imzası** denetlenmiyor (RFC 8058 bunu ister). Sahte bir gönderici, kullanıcı "Abonelikten çık"a basınca genel bir HTTPS adresine tek tık isteği ya da bir adrese çıkma postası gönderilmesini sağlayabilir; bu da adresin kullanıldığını ele verir.
- **Bellekteki veri:** çözülmüş veritabanı sayfaları ve anahtar zinciri kitaplığının kopyaları süreç belleğinde kalabilir.
- **Lisans:** bazı bağımlılıklar MPL-2.0 (Tauri üzerinden cssparser/selectors, dirs üzerinden option-ext). GPL değil, ama CLAUDE.md'deki izin listesinde yok; karar sahibin.
