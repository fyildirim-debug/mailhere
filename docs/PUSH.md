# Sunucu itmesi (push)

Telefon uykudayken ve Mailhere kapalıyken de yeni posta saniyeler içinde bildirilsin diye posta sunucusu, Mailhere bildirim aktarıcısı (`crates/mh-relay`) ve Firebase Cloud Messaging (FCM) birlikte çalışır. Aktarıcı yalnız "şu abone değişti" sinyali taşır: parola, erişim jetonu, posta adresi (JMAP'te) ya da posta içeriği görmez ve saklamaz. Posta her zaman telefondan doğrudan posta sunucusundan iner.

```
Posta sunucusu ──(yeni posta)──▶ Aktarıcı ──(FCM veri iletisi: "değişti")──▶ Google ──▶ Telefon
       ▲                                                                                 │
       └──────────────────────── telefon postayı kendisi indirir ◀──────────────────────┘
```

İtme yoksa (Firebase ayarı alınamadı, Google Play Hizmetleri yok, sağlayıcı kapalı) uygulama her zamanki yoldan çalışır: ön plan hizmeti (IMAP IDLE / JMAP akışı) ve 15 dakikalık yoklama işi.

## Sağlayıcılar

| Sağlayıcı | Durum | Sunucunun haber yolu | Uygulamanın yapacağı |
|---|---|---|---|
| JMAP (Stalwart) | **Açık** | `PushSubscription` (RFC 8620 §7.2) aktarıcının `/v1/jmap/{id}/{anahtar}` ucuna | Abonelik kur, doğrula, 7 günde bir uzat |
| IMAP, sunucu JMAP de konuşuyorsa (Stalwart) | **Açık** | Aynı `PushSubscription`, IMAP sunucusunun kendi adındaki JMAP ucunda (`https://<IMAP sunucusu>/.well-known/jmap`, IMAP kullanıcı adı ve parolası) | Abonelik JMAP'te, eşitleme IMAP'te; itme IMAP eşitlemesini tetikler. Parola başka ada gitmez; JMAP'i olmayan sunucu 7 gün yeniden denenmez. Yalnız parolalı hesaplar |
| Microsoft Graph (Outlook) | Hazır, kapalı | Graph aboneliği `/v1/graph/{id}` ucuna (`clientState` eşleşir) | `Mail.ReadBasic` izni + Graph aboneliği (3 günde bir yenile) |
| Gmail | Hazır, kapalı | Gmail API `users.watch` → Google Pub/Sub → `/v1/gmail` (Google imzalı) | Gmail API izni + `users.watch` (7 günde bir yenile) |

Kapalı sağlayıcının aktarıcı uçları 404 döner; açmak için aktarıcıda `MH_PROVIDERS` ve uygulamada `mh_app::push::saglayici` güncellenir. Gmail ve Graph'ın uygulama tarafı OAuth izinleri gelince eklenir (Google'ın kısıtlı kapsam onayı, Microsoft uygulama kaydı).

## Uygulama açıkken: JMAP durum akışı (IMAP hesapları)

Uygulama çalışırken (masaüstünde tepside, Android'de ön plan hizmetinde) aktarıcıya gerek yoktur: `mh_app::canli` JMAP konuşan sunucudaki her parolalı IMAP hesabı için sunucunun durum akışına (RFC 8620 §7.3, EventSource, `types=EmailDelivery`; sunucu tanımazsa `Email`) bağlanır. Yeni posta teslim edilince IMAP eşitlemesi hemen başlar; IDLE kopmuş ya da geç fark edilmiş olsa da posta saniyeler içinde gelir. Adres ve kimlik itme aboneliğiyle aynıdır (IMAP sunucusunun kendi adı, IMAP kullanıcı adı ve parolası). JMAP'i olmayan sunucu 6 saat denenmez; kopan akış 2 sn'den 5 dk'ya artan beklemeyle yenilenir. Hesap listesi dakikada bir gözden geçirilir.

## Akış (JMAP)

1. Android kabuğu Firebase'i aktarıcıdan aldığı istemci ayarıyla başlatır (`/v1/yapilandirma`; APK'da ve depoda Firebase anahtarı yoktur), kayıt jetonunu kasaya `push-jeton` adıyla yazar (`Itme.kt`).
2. Çekirdek açılınca, hesap eklenince ve 6 saatte bir `mh_app::push::Parcalar::refresh` her JMAP hesabı (ve JMAP'i olan sunucudaki IMAP hesabı) için aktarıcıda abone açar (`POST /v1/abone` → kimlik, yönetim anahtarı, sunucuya verilecek adres) ve posta sunucusunda `PushSubscription` kurar (`types: ["EmailDelivery"]`, tanımazsa `["Email"]`; `expires` 7 gün).
3. Sunucu adrese `PushVerification` yollar; aktarıcı kodu FCM ile telefona iletir (`t=dogrula`), çekirdek kodu sunucuya verir (`PushSubscription/set` → `verificationCode`). Çekirdek kapalıysa kod kasada (`push-bekleyen`) bekler, çekirdek açılınca uygulanır.
4. Yeni posta: sunucu `StateChange` yollar → aktarıcı `t=degisti` iletir (aynı aboneye 2 sn içindeki değişiklikler birleşir, sondaki kaçmaz) → `ItmeServisi` çekirdek açıksa hesabı hemen eşitletir (işlemci 30 sn uyanık), değilse hızlandırılmış iş açar (hizmet ya da tek seferlik eşitle-bildir). Bildirim her zamanki koddan geçer (kurallar, elek, sessiz saat).
5. Jeton yenilenince aboneler aktarıcıda yeni jetona taşınır; silinen hesabın abonesi silinir; aktarıcıya haftada bir canlılık bildirilir (30 gün sessiz abone orada silinir; Firebase "kayıtsız jeton" derse hemen).

## Aktarıcı

Rust (hyper), tek ikili `mailhere-relay`, aboneler `/data/aboneler.json` dosyasında (atomik yazım). Uçlar ve sınırlar `crates/mh-relay/src/sunucu.rs` başında.

| Ortam değişkeni | Anlamı |
|---|---|
| `MH_RELAY_PUBLIC_URL` | Dış adres (ör. `https://mailhere-push.arnexlab.com`) |
| `MH_FCM_SERVICE_ACCOUNT` | Firebase hizmet hesabı JSON'u (Kasa: "Mailhere FCM hizmet hesabı") |
| `MH_FIREBASE_API_KEY`, `MH_FIREBASE_APP_ID`, `MH_FIREBASE_SENDER_ID` | Uygulamaya verilen istemci ayarı (Kasa: "Mailhere google-services") |
| `MH_PROVIDERS` | Etkin sağlayıcılar, varsayılan `jmap` |
| `MH_GMAIL_AUDIENCE`, `MH_GMAIL_PUSH_SA`, `MH_GMAIL_TOPIC` | Gmail açıksa: Pub/Sub OIDC hedef kitlesi, itme hizmet hesabı, konu |
| `MH_RELAY_DATA`, `PORT` | Veri klasörü (`/data`), kapı (8080) |

Dağıtım: Dokploy uygulaması, kaynak bu depo (`main`), `crates/mh-relay/Dockerfile` (bağlam depo kökü), `/data` kalıcı birim, alan adı Let's Encrypt ile. Yalnız `crates/mh-relay/**` değişince yeniden kurulur.

Yerel deneme:

```bash
cargo test -p mh-relay
MH_RELAY_PUBLIC_URL=http://localhost:8080 MH_FCM_SERVICE_ACCOUNT="$(cat hizmet-hesabi.json)" \
  MH_RELAY_DATA=/tmp/relay cargo run -p mh-relay
MAILHERE_RELAY_URL=http://localhost:8080 pnpm tauri android dev   # uygulama yerel aktarıcıya
```
