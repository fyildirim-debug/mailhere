//! Kural komutları (`rules_list`, `rule_save`, `rule_delete`,
//! `rules_reorder`, `rule_run`) ve klasöre taşıma (`messages_move_to`).

use mh_core::{
    Address, AppError, AppResult, ErrorCode, MessageId, OutgoingMessage, Rule, RuleAction,
    RuleCondition,
};
use mh_store::{Store, MAX_RULES};
use tauri::{AppHandle, State};

use crate::rules::Engines;
use crate::state::{internal, AppState};

type Res<T> = Result<T, AppError>;

/// Kural adının ve metin değerlerinin üst sınırı (karakter).
pub(crate) const NAME_MAX: usize = 80;
pub(crate) const VALUE_MAX: usize = 500;
pub(crate) const CONDITIONS_MAX: usize = 20;
pub(crate) const ACTIONS_MAX: usize = 10;
/// E-posta adresinin üst sınırı (RFC 5321).
const EMAIL_MAX: usize = 254;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

fn rule_gone() -> AppError {
    AppError::new(ErrorCode::NotFound, "Kural bulunamadı; silinmiş olabilir.")
}

/// Denetim karakterleri atılmış, kırpılmış ve `max` karaktere kısaltılmış metin.
pub(crate) fn clip(s: &str, max: usize) -> String {
    let line = mh_mime::clean_line(s);
    let cut: String = line.trim().chars().take(max).collect();
    cut.trim_end().to_owned()
}

/// Tek ve geçerli bir e-posta adresi mi (gönderimde kabul edilecek biçimde)?
pub(crate) fn valid_email(email: &str) -> bool {
    if email.len() > EMAIL_MAX
        || email.matches('@').count() != 1
        || !mh_store::is_full_address(email)
        || email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>()[],;:\"\\".contains(c))
    {
        return false;
    }
    let probe = OutgoingMessage {
        account_id: String::new(),
        to: vec![Address::new(None, email)],
        cc: vec![],
        bcc: vec![],
        subject: String::new(),
        body_text: String::new(),
        in_reply_to: None,
        attachments: vec![],
        send_at: None,
        forward_from: None,
        draft_id: None,
        follow_up_at: None,
    };
    mh_send::validate(&probe).is_ok()
}

/// Kuralı denetler ve sadeleştirir: ad ve değerler kırpılır ve kısaltılır;
/// koşul ve eylem sayısı, hesap, klasör ve iletme adresi denetlenir.
pub fn normalize_rule(store: &Store, mut rule: Rule) -> AppResult<Rule> {
    rule.id = rule.id.trim().to_owned();
    rule.name = clip(&rule.name, NAME_MAX);
    if rule.name.is_empty() {
        return Err(invalid("Kurala bir ad ver."));
    }
    if rule.conditions.is_empty() || rule.conditions.len() > CONDITIONS_MAX {
        return Err(invalid("Kuralın 1 ile 20 arasında koşulu olmalı."));
    }
    if rule.actions.is_empty() || rule.actions.len() > ACTIONS_MAX {
        return Err(invalid("Kuralın 1 ile 10 arasında eylemi olmalı."));
    }
    for c in &mut rule.conditions {
        let value = match c {
            RuleCondition::From { value, .. }
            | RuleCondition::To { value, .. }
            | RuleCondition::Subject { value, .. }
            | RuleCondition::Body { value, .. } => value,
            RuleCondition::HasAttachment | RuleCondition::Category { .. } => continue,
        };
        *value = clip(value, VALUE_MAX);
        if value.is_empty() {
            return Err(invalid("Koşulların değeri boş olamaz."));
        }
    }
    rule.account_id = rule
        .account_id
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty());
    if let Some(a) = &rule.account_id {
        if store.account(a)?.is_none() {
            return Err(invalid(
                "Kuralın hesabı bulunamadı; kaldırılmış olabilir. Başka bir hesap seç.",
            ));
        }
    }
    for a in &mut rule.actions {
        match a {
            RuleAction::Move { mailbox_id } => {
                let Some(account) = rule.account_id.as_deref() else {
                    return Err(invalid("Klasöre taşımak için kurala bir hesap seç."));
                };
                *mailbox_id = mailbox_id.trim().to_owned();
                let ok = store
                    .mailbox(mailbox_id)?
                    .is_some_and(|m| m.mailbox.account_id == account);
                if !ok {
                    return Err(invalid("Taşınacak klasör bu hesapta yok."));
                }
            }
            RuleAction::Forward { to } => {
                *to = to.trim().to_owned();
                if !valid_email(to) {
                    return Err(invalid(
                        "İletilecek adres geçerli bir e-posta adresi olmalı.",
                    ));
                }
            }
            RuleAction::Tag { tag_id } => {
                *tag_id = tag_id.trim().to_owned();
                if store.tag(tag_id)?.is_none() {
                    return Err(invalid(
                        "Eklenecek etiket bulunamadı; silinmiş olabilir. Başka bir etiket seç.",
                    ));
                }
            }
            RuleAction::Archive
            | RuleAction::Trash
            | RuleAction::MarkRead
            | RuleAction::Star
            | RuleAction::Mute => {}
        }
    }
    Ok(rule)
}

/// Kuralı denetleyip kaydeder: kimliği boşsa yeni kimlikle sona eklenir,
/// değilse yerinde güncellenir.
pub fn save_rule(store: &Store, rule: Rule) -> AppResult<Rule> {
    let mut rule = normalize_rule(store, rule)?;
    if rule.id.is_empty() {
        if store.rule_count()? >= MAX_RULES {
            return Err(invalid("En çok 200 kural olabilir. Kullanmadıklarını sil."));
        }
        rule.id = uuid::Uuid::new_v4().to_string();
        store.rule_insert(&rule)?;
    } else if !store.rule_update(&rule)? {
        return Err(rule_gone());
    }
    Ok(rule)
}

#[tauri::command]
pub fn rules_list(state: State<'_, AppState>) -> Res<Vec<Rule>> {
    state.store.rules()
}

#[tauri::command]
pub fn rule_save(state: State<'_, AppState>, rule: Rule) -> Res<Rule> {
    save_rule(&state.store, rule)
}

/// Kuralı siler; zaten yoksa sessizce geçer.
#[tauri::command]
pub fn rule_delete(state: State<'_, AppState>, id: String) -> Res<()> {
    state.store.rule_delete(id.trim())?;
    Ok(())
}

#[tauri::command]
pub fn rules_reorder(state: State<'_, AppState>, ids: Vec<String>) -> Res<()> {
    state.store.rules_reorder(&ids)
}

/// Kuralı ilgili hesap(lar)ın gelen kutusundaki mevcut postalara uygular
/// (iletme ve susturma hariç; kapalı kural da çalıştırılabilir). Eşleşen
/// posta sayısını döner.
#[tauri::command]
pub async fn rule_run(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<u32> {
    let rule = state.store.rule(id.trim())?.ok_or_else(rule_gone)?;
    let store = state.store.clone();
    let scanned = rule.clone();
    // Büyük gelen kutusunda tarama uzun sürebilir: çalışma iş parçacıklarını tutmasın.
    let matched = tokio::task::spawn_blocking(move || crate::rules::scan_inbox(&store, &scanned))
        .await
        .map_err(internal)??;
    let touched = Engines::of(&state).apply_rule(&rule, &matched)?;
    if !touched.is_empty() {
        crate::sink::changed(&app, touched);
    }
    Ok(matched.len() as u32)
}

/// Postaları hesabının bir klasörüne taşır (erteleme kalkar).
#[tauri::command]
pub fn messages_move_to(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    mailbox_id: String,
) -> Res<()> {
    let touched = Engines::of(&state).move_to_mailbox(&ids, &mailbox_id)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

#[cfg(test)]
mod tests {
    use mh_core::{
        Account, AccountKind, Category, MailboxRole, RuleMatch, RuleOp, Security, ServerConfig,
    };

    use super::*;

    fn store() -> Store {
        let s = Store::open_in_memory().unwrap();
        for id in ["a", "b"] {
            s.insert_account(&Account {
                id: id.into(),
                name: id.into(),
                email: format!("{id}@ornek.com"),
                display_name: id.into(),
                color: "#4C6FD8".into(),
                kind: AccountKind::Imap,
                imap: Some(ServerConfig {
                    host: "imap.ornek.invalid".into(),
                    port: 993,
                    security: Security::Tls,
                    username: id.into(),
                }),
                smtp: None,
                jmap_url: None,
                signatures: Vec::new(),
                signature_new: None,
                signature_reply: None,
                created_at: 0,
                auth: Default::default(),
                notify: true,
            })
            .unwrap();
            s.upsert_mailbox(
                id,
                "Faturalar",
                "Faturalar",
                MailboxRole::Other,
                None,
                false,
            )
            .unwrap();
        }
        s
    }

    fn rule() -> Rule {
        Rule {
            id: String::new(),
            name: "  Faturalar\n".into(),
            enabled: true,
            account_id: Some(" a ".into()),
            match_: RuleMatch::All,
            conditions: vec![
                RuleCondition::From {
                    op: RuleOp::Contains,
                    value: "  muhasebe\t".into(),
                },
                RuleCondition::Category {
                    value: Category::Notifications,
                },
            ],
            actions: vec![
                RuleAction::Move {
                    mailbox_id: "a:Faturalar".into(),
                },
                RuleAction::Forward {
                    to: " Muhasebe@Sirket.com ".into(),
                },
            ],
            stop: false,
        }
    }

    fn err(s: &Store, r: Rule) -> String {
        let e = save_rule(s, r).unwrap_err();
        assert_eq!(e.code, ErrorCode::Invalid, "{}", e.message);
        e.message
    }

    #[test]
    fn kural_sadelestirilip_kaydedilir() {
        let s = store();
        let saved = save_rule(&s, rule()).unwrap();
        assert!(!saved.id.is_empty(), "yeni kimlik");
        assert_eq!(saved.name, "Faturalar");
        assert_eq!(saved.account_id.as_deref(), Some("a"));
        assert_eq!(
            saved.conditions[0],
            RuleCondition::From {
                op: RuleOp::Contains,
                value: "muhasebe".into()
            }
        );
        assert_eq!(
            saved.actions[1],
            RuleAction::Forward {
                to: "Muhasebe@Sirket.com".into()
            }
        );
        assert_eq!(s.rules().unwrap(), vec![saved.clone()]);

        // Güncelleme yerinde; ad ve değer kısaltılır.
        let mut edit = saved.clone();
        edit.name = "ş".repeat(100);
        edit.conditions[0] = RuleCondition::Subject {
            op: RuleOp::Is,
            value: "x".repeat(600),
        };
        let edited = save_rule(&s, edit).unwrap();
        assert_eq!(edited.id, saved.id);
        assert_eq!(edited.name.chars().count(), 80);
        let RuleCondition::Subject { value, .. } = &edited.conditions[0] else {
            panic!()
        };
        assert_eq!(value.len(), 500);
        assert_eq!(s.rules().unwrap().len(), 1);

        // Bilinmeyen kimlik.
        let mut ghost = saved;
        ghost.id = "yok".into();
        let e = save_rule(&s, ghost).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(e.message, "Kural bulunamadı; silinmiş olabilir.");
    }

    #[test]
    fn gecersiz_kurallar() {
        let s = store();
        let with = |f: &dyn Fn(&mut Rule)| {
            let mut r = rule();
            f(&mut r);
            r
        };
        assert_eq!(
            err(&s, with(&|r| r.name = " \t ".into())),
            "Kurala bir ad ver."
        );
        assert_eq!(
            err(&s, with(&|r| r.conditions.clear())),
            "Kuralın 1 ile 20 arasında koşulu olmalı."
        );
        assert_eq!(
            err(
                &s,
                with(&|r| r.conditions = vec![RuleCondition::HasAttachment; 21])
            ),
            "Kuralın 1 ile 20 arasında koşulu olmalı."
        );
        assert_eq!(
            err(&s, with(&|r| r.actions.clear())),
            "Kuralın 1 ile 10 arasında eylemi olmalı."
        );
        assert_eq!(
            err(&s, with(&|r| r.actions = vec![RuleAction::Star; 11])),
            "Kuralın 1 ile 10 arasında eylemi olmalı."
        );
        assert_eq!(
            err(
                &s,
                with(&|r| r.conditions[0] = RuleCondition::Body {
                    op: RuleOp::NotContains,
                    value: " \u{200F} ".into()
                })
            ),
            "Koşulların değeri boş olamaz."
        );
        assert_eq!(
            err(&s, with(&|r| r.account_id = Some("silinmis".into()))),
            "Kuralın hesabı bulunamadı; kaldırılmış olabilir. Başka bir hesap seç."
        );
        assert_eq!(
            err(&s, with(&|r| r.account_id = None)),
            "Klasöre taşımak için kurala bir hesap seç."
        );
        assert_eq!(
            err(&s, with(&|r| r.account_id = Some(" ".into()))),
            "Klasöre taşımak için kurala bir hesap seç."
        );
        assert_eq!(
            err(&s, with(&|r| r.account_id = Some("b".into()))),
            "Taşınacak klasör bu hesapta yok."
        );
        assert_eq!(
            err(
                &s,
                with(&|r| r.actions[0] = RuleAction::Move {
                    mailbox_id: "a:Yok".into()
                })
            ),
            "Taşınacak klasör bu hesapta yok."
        );
        let long = format!("{}@ornek.com", "a".repeat(250));
        for bad in [
            "",
            "muhasebe",
            "a@com",
            "a@@b.com",
            "a@b.com, c@d.com",
            "Ayşe <a@b.com>",
            "a b@c.com",
            "a@b.com\r\nBcc: x@y.com",
            long.as_str(),
        ] {
            assert_eq!(
                err(
                    &s,
                    with(&|r| r.actions[1] = RuleAction::Forward { to: bad.into() })
                ),
                "İletilecek adres geçerli bir e-posta adresi olmalı.",
                "{bad}"
            );
        }
        // Hesapsız kural: taşıma dışındaki eylemler serbest.
        let ok = with(&|r| {
            r.account_id = None;
            r.actions = vec![RuleAction::Archive, RuleAction::Mute];
        });
        save_rule(&s, ok).unwrap();
        // Etiket eylemi var olan bir etiketi ister; kimlik kırpılır.
        assert_eq!(
            err(
                &s,
                with(&|r| r.actions = vec![RuleAction::Tag {
                    tag_id: "yok".into()
                }])
            ),
            "Eklenecek etiket bulunamadı; silinmiş olabilir. Başka bir etiket seç."
        );
        s.tag_insert(&mh_core::Tag {
            id: "e1".into(),
            name: "Fatura".into(),
            color: "#4C6FD8".into(),
        })
        .unwrap();
        let saved = save_rule(
            &s,
            with(&|r| {
                r.actions = vec![RuleAction::Tag {
                    tag_id: " e1 ".into(),
                }]
            }),
        )
        .unwrap();
        assert_eq!(
            saved.actions,
            vec![RuleAction::Tag {
                tag_id: "e1".into()
            }]
        );
    }

    #[test]
    fn en_cok_200_kural() {
        let s = store();
        for _ in 0..MAX_RULES {
            save_rule(&s, rule()).unwrap();
        }
        assert_eq!(
            err(&s, rule()),
            "En çok 200 kural olabilir. Kullanmadıklarını sil."
        );
        // Var olan kural yine güncellenebilir.
        let first = s.rules().unwrap().remove(0);
        save_rule(&s, first).unwrap();
    }
}
