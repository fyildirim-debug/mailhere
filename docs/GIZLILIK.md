# Mailhere Gizlilik Politikası

*Son güncelleme: 22 Kasım 2026*

Mailhere (Microsoft Store'da **MailHere**), Furkan YILDIRIM tarafından geliştirilen açık kaynaklı (MIT) bir e-posta istemcisidir. Bu politika Windows, Linux ve Android sürümleri için geçerlidir. [English version below.](#mailhere-privacy-policy)

## Kısaca

- Mailhere'in kendi sunucusu yoktur; postaların, kişilerin ve hesap bilgilerin bize gönderilmez.
- Reklam, izleme, analiz ya da çökme raporu toplanmaz.
- Veriler yalnız senin cihazında, şifreli olarak durur.

## Hangi veriler işlenir

**Posta hesabı bilgileri.** Eklediğin hesabın adresi, sunucu ayarları ve parolası ya da oturum jetonu (Google, Microsoft girişi) yalnız cihazında tutulur. Parolalar ve jetonlar işletim sisteminin anahtar zincirinde (Windows Kimlik Bilgileri Yöneticisi, Linux Secret Service, Android Keystore) saklanır.

**Postalar.** Postalar doğrudan senin posta sunucundan (IMAP, SMTP, JMAP) indirilir ve gönderilir. Hızlı arama ve çevrimdışı okuma için cihazdaki yerel veritabanına yazılır. Bu veritabanı AES-256 ile şifrelidir; anahtarı yalnız cihazın anahtar zincirindedir.

**Ayarlar.** Uygulama ayarların cihazda saklanır.

## Üçüncü taraflarla bağlantı

- **Posta sağlayıcın:** Mailhere yalnız senin seçtiğin posta sunucusuna bağlanır. O sunucunun gizlilik politikası ayrıca geçerlidir.
- **Google ve Microsoft girişi:** Bu yolla hesap eklersen giriş Google ya da Microsoft'un sayfasında yapılır; Mailhere yalnız posta erişimi için verilen jetonu cihazında saklar.
- **Uzak içerik:** Postalardaki uzak görseller varsayılan olarak yüklenmez. Sen yüklemeyi seçersen görselin bulunduğu sunucu IP adresini görebilir.
- **Güncelleme denetimi:** Microsoft Store dışındaki kurulumlarda Mailhere yeni sürümü GitHub'dan denetler. Bu istekte kişisel veri gönderilmez. Store kurulumunu Microsoft Store günceller.
- **Android anlık bildirim (isteğe bağlı):** Desteklenen sunucularda yeni posta haberinin telefon uykudayken de gelmesi için Mailhere bildirim aktarıcısı ve Google Firebase Cloud Messaging kullanılır. Aktarıcı yalnız cihazın bildirim jetonunu ve "bu abonelikte değişiklik var" sinyalini taşır; parola, jeton, posta içeriği, gönderen ya da konu almaz. 30 gün kullanılmayan abonelik silinir.

## Verilerin silinmesi

Bir hesabı Mailhere'den kaldırdığında o hesabın postaları yerel veritabanından, parolası ya da jetonu anahtar zincirinden silinir. Microsoft Store kurulumunu kaldırdığında Windows yerel verileri de siler; diğer kurulumlarda veriler uygulamanın veri klasöründen elle silinebilir.

## Çocuklar

Mailhere çocuklara yönelik değildir ve bilerek çocuklardan veri toplamaz; zaten hiçbir kullanıcıdan veri toplamaz.

## Değişiklikler ve iletişim

Bu politika değişirse güncel hali bu sayfada yayımlanır. Soruların için: posta@furkanyildirim.com

---

# Mailhere Privacy Policy

*Last updated: 22 November 2026*

Mailhere (**MailHere** on the Microsoft Store) is an open-source (MIT) email client developed by Furkan YILDIRIM. This policy applies to the Windows, Linux and Android versions.

## In short

- Mailhere has no server of its own; your mail, contacts and account details are never sent to us.
- No advertising, tracking, analytics or crash reports are collected.
- Your data stays on your device, encrypted.

## What data is processed

**Mail account details.** The address, server settings and password or sign-in token (Google, Microsoft) of the accounts you add are kept only on your device. Passwords and tokens are stored in the operating system's keychain (Windows Credential Manager, Linux Secret Service, Android Keystore).

**Mail.** Messages are downloaded from and sent through your own mail server (IMAP, SMTP, JMAP). They are stored in a local database on your device for fast search and offline reading. This database is encrypted with AES-256; its key is kept only in your device's keychain.

**Settings.** App settings are stored on your device.

## Connections to third parties

- **Your mail provider:** Mailhere connects only to the mail server you choose. That provider's own privacy policy also applies.
- **Google and Microsoft sign-in:** If you add an account this way, you sign in on Google's or Microsoft's own page; Mailhere only stores the resulting mail-access token on your device.
- **Remote content:** Remote images in messages are not loaded by default. If you choose to load them, the server hosting the image can see your IP address.
- **Update checks:** Outside the Microsoft Store, Mailhere checks GitHub for new versions. No personal data is sent. Store installations are updated by the Microsoft Store.
- **Android instant notifications (optional):** On supported servers, the Mailhere notification relay and Google Firebase Cloud Messaging are used so that new-mail alerts arrive even while the phone sleeps. The relay only handles the device's notification token and a "this subscription changed" signal; it never receives passwords, tokens, message content, senders or subjects. Subscriptions unused for 30 days are deleted.

## Deleting your data

When you remove an account from Mailhere, its messages are deleted from the local database and its password or token from the keychain. Uninstalling the Microsoft Store version makes Windows delete the local data too; for other installations the data can be deleted from the app's data folder.

## Children

Mailhere is not directed at children and does not knowingly collect data from children; it does not collect data from any user.

## Changes and contact

If this policy changes, the current version will be published on this page. Questions: posta@furkanyildirim.com
