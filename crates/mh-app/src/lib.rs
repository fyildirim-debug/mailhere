//! Mailhere ortak uygulama katmanı: masaüstü (`src-tauri`) ve Android
//! (`mailheremobile`) kabuklarının paylaştığı komutlar, durum, olay köprüsü
//! ve platform sözleşmesi.
//!
//! Kabuğa özgü iş (pencere, tepsi, bildirim kartı, dosya penceresi, anahtar
//! zinciri) burada yoktur; [`Platform`] üzerinden kabuğa bırakılır. Kabuk
//! açılışta [`setup_core`] çağırır ve komutları [`shared_commands!`] ile
//! kendi komutlarına ekler.

pub mod cmd_account;
pub mod cmd_cert;
pub mod cmd_drafts;
pub mod cmd_images;
pub mod cmd_jmap;
pub mod cmd_list;
pub mod cmd_mailbox;
pub mod cmd_oauth;
pub mod cmd_outbox;
pub mod cmd_reading;
pub mod cmd_rules;
pub mod cmd_unsubscribe;
pub mod cmd_vacation;
pub mod commands;
pub mod files;
pub mod followups;
pub mod logging;
pub mod notify;
pub mod platform;
pub mod routes;
pub mod rules;
pub mod screener;
pub mod signatures;
pub mod sink;
pub mod state;
pub mod transfer;
pub mod wake;

pub use platform::{BoxFut, Confirm, Platform};
pub use state::{setup_core, AppState, Dirs};

/// Ortak komut listesi tek yerden: hem kabukların `tauri::generate_handler!`
/// çağrısına giren yollar (`shared_commands!`) hem de adların dizisi
/// (`SHARED_COMMANDS`, `commands.txt` ile eşlenir) buradan üretilir.
///
/// İlk parametre `$` işaretinin kendisidir: iç makronun kendi değişkenlerini
/// (`$extra`) dış makro yorumlamasın diye dolar işareti belirteç olarak geçer.
macro_rules! define_shared_commands {
    ($d:tt $($module:ident :: $name:ident),* $(,)?) => {
        /// Ortak komut adları, `shared_commands!` ile aynı sırada.
        pub const SHARED_COMMANDS: &[&str] = &[$(stringify!($name)),*];

        /// Ortak komutlar + kabuğun kendi komutları için `invoke_handler`.
        ///
        /// ```ignore
        /// .invoke_handler(mh_app::shared_commands![windows::open_quick, updater::update_check])
        /// ```
        #[macro_export]
        macro_rules! shared_commands {
            ($d($d extra:path),* $d(,)?) => {
                ::tauri::generate_handler![$($crate::$module::$name,)* $d($d extra),*]
            };
        }
    };
}

define_shared_commands! { $
    commands::accounts_list,
    commands::account_discover,
    commands::account_add,
    commands::account_remove,
    commands::mailboxes_list,
    commands::counts,
    commands::messages_list,
    commands::message_get,
    commands::messages_flag,
    commands::messages_move,
    commands::messages_snooze,
    commands::search,
    commands::remote_allow,
    commands::attachment_open,
    commands::send,
    commands::send_cancel,
    commands::settings_get,
    commands::settings_set,
    commands::quick_replies_set,
    commands::sync_now,
    commands::sync_reconnect,
    commands::open_main,
    commands::open_compose,
    commands::notify_snooze,
    cmd_jmap::jmap_discover,
    cmd_jmap::account_add_jmap,
    cmd_account::account_update,
    cmd_account::account_update_server,
    cmd_account::account_set_password,
    cmd_mailbox::mailbox_set_hidden,
    cmd_mailbox::mailbox_set_role,
    cmd_mailbox::mailbox_roles_reset,
    cmd_drafts::draft_save,
    cmd_drafts::draft_delete,
    files::attachments_pick,
    cmd_oauth::oauth_providers,
    cmd_oauth::oauth_sign_in,
    cmd_oauth::account_add_oauth,
    cmd_cert::cert_inspect,
    cmd_cert::cert_trust,
    cmd_unsubscribe::unsubscribe,
    cmd_vacation::vacation_get,
    cmd_vacation::vacation_set,
    cmd_rules::rules_list,
    cmd_rules::rule_save,
    cmd_rules::rule_delete,
    cmd_rules::rules_reorder,
    cmd_rules::rule_run,
    cmd_rules::messages_move_to,
    cmd_outbox::outbox_list,
    cmd_outbox::outbox_send_now,
    cmd_outbox::outbox_to_draft,
    cmd_reading::thread_get,
    cmd_reading::contact_get,
    cmd_reading::message_save_eml,
    cmd_reading::invite_reply,
    cmd_reading::invite_open,
    followups::followup_set,
    followups::followups_list,
    screener::screener_list,
    screener::screener_decide,
    cmd_list::messages_by_sender,
    cmd_list::messages_pin,
    cmd_list::tags_list,
    cmd_list::tag_save,
    cmd_list::tag_delete,
    cmd_list::messages_tag,
    transfer::transfer_begin,
    transfer::transfer_cancel,
    transfer::transfer_receive,
    transfer::transfer_apply,
    cmd_images::remote_images,
}

/// `commands.txt` içindeki adlar (yorum ve boş satırlar atılır).
pub fn commands_txt() -> Vec<&'static str> {
    include_str!("../commands.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn komut_listesi_tek_kaynak() {
        assert_eq!(
            commands_txt(),
            SHARED_COMMANDS,
            "commands.txt ile shared_commands! listesi aynı sırada aynı adları taşımalı"
        );
        let mut sorted = SHARED_COMMANDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), SHARED_COMMANDS.len(), "yinelenen komut adı");
        for name in SHARED_COMMANDS {
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
                "{name}: komut adı snake_case olmalı"
            );
        }
    }
}
