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
- Koyu temada postanın yalnız yazı renkleri ana pencereden uyarlanır; çerçevede yine betik çalışmaz, içerik eklenmez.
- Ekler yalnız izin listesindeki türlerde doğrudan açılır, diğerleri klasörde gösterilir. Windows'ta "İnternet'ten geldi" işareti konur. Dosya adı kısaltılırken uzantı korunur.
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

## Açık kalanlar

- **GitHub ayarı:** `yayin` ortamına onaylayıcı eklenmeli (Settings → Environments). İlk sürümde ortam kendiliğinden oluşur.
- **CI eylemleri** sürüm etiketine bağlı, commit özetine (SHA) değil. Dependabot günceller; daha sıkı isteniyorsa SHA'ya sabitlenebilir.
- **Uzak görsel izni** gönderen adresine göre veriliyor. Gönderen adresi sahtelenebilir, bu yüzden izinli bir gönderici taklit edilirse izleme pikseli yüklenebilir. DMARC doğrulaması ileride eklenebilir.
- **Karışık alfabeli alan adları** (Kiril "а" gibi) ayrıca işaretlenmiyor.
- **Abonelikten çıkma başlığının DKIM imzası** denetlenmiyor (RFC 8058 bunu ister). Sahte bir gönderici, kullanıcı "Abonelikten çık"a basınca genel bir HTTPS adresine tek tık isteği ya da bir adrese çıkma postası gönderilmesini sağlayabilir; bu da adresin kullanıldığını ele verir.
- **Bellekteki veri:** çözülmüş veritabanı sayfaları ve anahtar zinciri kitaplığının kopyaları süreç belleğinde kalabilir.
- **Lisans:** bazı bağımlılıklar MPL-2.0 (Tauri üzerinden cssparser/selectors, dirs üzerinden option-ext). GPL değil, ama CLAUDE.md'deki izin listesinde yok; karar sahibin.
