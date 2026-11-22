# Microsoft Store (MSIX)

Mailhere, Microsoft Store'da **MailHere** adıyla (Partner Center ürün kimliği `9P7H3KWKXLVT`) MSIX paketi olarak yayımlanır.

- **Kimlik:** `ArnexLab.MailHere`, yayımcı `CN=CAE57BB8-AC80-4682-AD33-4FBB0E661588`, görünen yayımcı `ArnexLab` (Partner Center › Ürün kimliği). Bildirim: `packaging/msix/AppxManifest.xml`.
- **Paket üretimi:** Actions › *Mağaza paketi* › Run workflow (`.github/workflows/magaza.yml`). İş `mailhere.exe`'yi derler, `packaging/msix/paketle.ps1` ile `makeappx` paketini yapar, geçici sertifikayla deneme kurulumu yapıp kaldırır ve `magaza-msix` çıktısını yükler. Paket sürümü `package.json` sürümü + `.0`.
- **Yükleme:** Çıktıdaki `MailHere_X.Y.Z.0_x64.msix` Partner Center › Gönderim › Paketler'e sürüklenir. Paket imzasızdır; Microsoft imzalar.
- **Mağaza kurulumunda davranış** (`src-tauri/src/package.rs` paket kimliğini algılar):
  - Uygulama içi güncelleyici kapalıdır; sürümü Mağaza günceller.
  - Açılışta başlama kayıt defteri yerine paketteki `MailhereStartup` görevi (`windows.startupTask`, `--tepside`) ile yönetilir (`autostart.rs` › `store_task`). Kullanıcı Görev Yöneticisi'nden kapatırsa uygulama yeniden açamaz; ayar Windows'tan açılır.
