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
- Ekler yalnız izin listesindeki türlerde doğrudan açılır, diğerleri klasörde gösterilir. Windows'ta "İnternet'ten geldi" işareti konur. Dosya adı kısaltılırken uzantı korunur.

**Uygulama kabuğu**
- Her pencere yalnız kullandığı komutları çağırabilir (`src-tauri/capabilities/*.json`, `build.rs`). Bildirim kartı ve hızlı okuma ayar, hesap ve sertifika komutlarına erişemez.
- Postaya yalnız kullanıcının işletim sisteminin dosya penceresinde seçtiği dosyalar eklenebilir. Arayüz kendi yol yazamaz.
- Pencereler uygulama dışı adrese yönlendirilemez. Dış bağlantılar yalnız `http`, `https` ve `mailto` ile, varsayılan tarayıcıda açılır.
- Deneme anahtarları (`MAILHERE_DEMO`, `MAILHERE_SECRETS=memory`) ve OAuth kimliğini çalışırken değiştirme yalnız geliştirme derlemesinde çalışır.

**Ağ ve kimlik doğrulama**
- TLS zorunlu; şifresiz bağlantı yalnız `localhost`'a. STARTTLS reddedilirse düz metne düşülmez.
- Geçersiz sertifika yalnız sunucunun gerçekten sunduğu sertifika için onaylanabilir. Onay ayrıca işletim sisteminin kendi penceresinde istenir. Büyük sağlayıcılarda (Gmail, Outlook…) onay verilemez.
- JMAP parolası yalnız oturumun kendi kökenine gider. Başka kökene, `http`'ye ya da bu makineye yönlendirmede gönderilmez. Sunucu yanıtları boyutla sınırlıdır.
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
- **Bellekteki veri:** çözülmüş veritabanı sayfaları ve anahtar zinciri kitaplığının kopyaları süreç belleğinde kalabilir.
- **Lisans:** bazı bağımlılıklar MPL-2.0 (Tauri üzerinden cssparser/selectors, dirs üzerinden option-ext). GPL değil, ama CLAUDE.md'deki izin listesinde yok; karar sahibin.
