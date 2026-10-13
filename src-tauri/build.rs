fn main() {
    // Uygulama komutları için izin tanımları: her pencere yalnız kendi
    // yetenek dosyasında (`capabilities/*.json`) izin verilen komutları
    // çağırabilir. Yeni bir komut buraya ve ilgili pencerenin dosyasına eklenir.
    const COMMANDS: &[&str] = &[
        "accounts_list",
        "account_discover",
        "account_add",
        "account_remove",
        "mailboxes_list",
        "counts",
        "messages_list",
        "message_get",
        "messages_flag",
        "messages_move",
        "messages_snooze",
        "search",
        "remote_allow",
        "attachment_open",
        "send",
        "send_cancel",
        "settings_get",
        "settings_set",
        "sync_now",
        "open_main",
        "open_quick",
        "open_compose",
        "notify_layout",
        "notify_hide",
        "notify_pending",
        "notify_snooze",
        "demo_notify",
        "jmap_discover",
        "account_add_jmap",
        "account_update",
        "draft_save",
        "draft_delete",
        "attachments_pick",
        "update_check",
        "update_install",
        "oauth_providers",
        "oauth_sign_in",
        "account_add_oauth",
        "cert_inspect",
        "cert_trust",
        "unsubscribe",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("Tauri derleme adımı başarısız");
}
