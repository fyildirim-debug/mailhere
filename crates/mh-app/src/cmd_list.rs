//! Liste komutları: göndericiye göre toplu seçim (`messages_by_sender`),
//! sabitleme (`messages_pin`), yerel etiketler (`tags_list`, `tag_save`,
//! `tag_delete`, `messages_tag`) ve kayıtlı aramaların sadeleştirilmesi.
//!
//! Sabitleme ve etiketler yalnız bu bilgisayarda tutulur; sunucuya yazılmaz.
//! Toplu arşivleme ve silmenin kendisi arayüzün çağırdığı mevcut
//! `messages_move` ile yapılır: burada yalnız kimlikler seçilir.

use mh_core::{
    AppError, AppResult, ErrorCode, MessageId, SavedSearch, Tag, View, SAVED_SEARCHES_MAX,
    SAVED_SEARCH_NAME_CHARS, SAVED_SEARCH_QUERY_CHARS, TAGS_MAX, TAG_COLORS,
};
use mh_store::{normalize_tag_name, SenderScope, Store};
use tauri::{AppHandle, State};

use crate::state::AppState;

type Res<T> = Result<T, AppError>;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::Invalid, message)
}

fn tag_gone() -> AppError {
    AppError::new(ErrorCode::NotFound, "Etiket bulunamadı; silinmiş olabilir.")
}

/// Gönderenin kapsamdaki (gelen kutusu, açık klasör ya da etiket) postaları,
/// yeniden eskiye. Arayüz bunları arşivler, çöpe atar ya da en yenisi dışında
/// arşivler.
#[tauri::command]
pub fn messages_by_sender(
    state: State<'_, AppState>,
    account_id: Option<String>,
    email: String,
    view: Option<View>,
) -> Res<Vec<MessageId>> {
    let account = account_id
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty());
    state
        .store
        .messages_by_sender(account, &email, &SenderScope::of(view.as_ref()))
}

/// Postaları sabitler ya da sabitlemeyi kaldırır (yalnız yerel).
#[tauri::command]
pub fn messages_pin(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    pinned: bool,
) -> Res<()> {
    let touched = state.store.set_pinned(&ids, pinned)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

#[tauri::command]
pub fn tags_list(state: State<'_, AppState>) -> Res<Vec<Tag>> {
    state.store.tags()
}

/// Etiketi denetler ve sadeleştirir: ad kırpılır ve kısaltılır, boş olamaz,
/// başka bir etiketle çakışamaz; renk paletten olmalı.
pub fn normalize_tag(store: &Store, mut tag: Tag) -> AppResult<Tag> {
    tag.id = tag.id.trim().to_owned();
    if !tag.id.is_empty() && store.tag(&tag.id)?.is_none() {
        return Err(tag_gone());
    }
    tag.name = normalize_tag_name(&tag.name);
    if tag.name.is_empty() {
        return Err(invalid("Etikete bir ad ver."));
    }
    let color = tag.color.trim().to_ascii_uppercase();
    let Some(color) = TAG_COLORS.iter().find(|c| c.eq_ignore_ascii_case(&color)) else {
        return Err(invalid("Etiket rengi paletten seçilmeli."));
    };
    tag.color = (*color).to_owned();
    let except = (!tag.id.is_empty()).then_some(tag.id.as_str());
    if store.tag_name_taken(&tag.name, except)? {
        return Err(invalid("Bu adda bir etiket zaten var."));
    }
    Ok(tag)
}

/// Etiketi denetleyip kaydeder: kimliği boşsa yeni kimlikle sona eklenir,
/// değilse yerinde güncellenir.
pub fn save_tag(store: &Store, tag: Tag) -> AppResult<Tag> {
    let mut tag = normalize_tag(store, tag)?;
    if tag.id.is_empty() {
        if store.tag_count()? >= TAGS_MAX {
            return Err(invalid("En çok 50 etiket olabilir. Kullanmadıklarını sil."));
        }
        tag.id = uuid::Uuid::new_v4().to_string();
        store.tag_insert(&tag)?;
    } else if !store.tag_update(&tag)? {
        return Err(tag_gone());
    }
    Ok(tag)
}

#[tauri::command]
pub fn tag_save(app: AppHandle, state: State<'_, AppState>, tag: Tag) -> Res<Tag> {
    let saved = save_tag(&state.store, tag)?;
    // Ad ya da renk değişti: listedeki çipler yenilensin.
    crate::sink::changed(&app, Vec::new());
    Ok(saved)
}

/// Etiketi siler; postalardan da kalkar. Zaten yoksa sessizce geçer.
#[tauri::command]
pub fn tag_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> Res<()> {
    let touched = state.store.tag_delete(id.trim())?;
    crate::sink::changed(&app, touched);
    Ok(())
}

/// Etiketi postalara ekler (`on`) ya da kaldırır.
#[tauri::command]
pub fn messages_tag(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<MessageId>,
    tag_id: String,
    on: bool,
) -> Res<()> {
    let touched = state.store.set_message_tag(&ids, tag_id.trim(), on)?;
    crate::sink::changed(&app, touched);
    Ok(())
}

/// Denetim karakterleri atılmış, kırpılmış ve `max` karaktere kısaltılmış tek
/// satır.
fn clip(s: &str, max: usize) -> String {
    let line = mh_mime::clean_line(s);
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = line.chars().take(max).collect();
    cut.trim_end().to_owned()
}

/// Kayıtlı aramaları sadeleştirir: ad en çok 60, sorgu en çok 300 karakter;
/// sorgusu boş olanlar atılır; adı boşsa sorgu ad olur; kimliği boş ya da
/// yinelenen kayıtlara yeni kimlik verilir; en çok 30 kayıt (ilk gelenler).
pub fn normalize_saved_searches(list: Vec<SavedSearch>) -> Vec<SavedSearch> {
    let mut out: Vec<SavedSearch> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in list {
        let query = clip(&s.query, SAVED_SEARCH_QUERY_CHARS);
        if query.is_empty() {
            continue;
        }
        let mut name = clip(&s.name, SAVED_SEARCH_NAME_CHARS);
        if name.is_empty() {
            name = query.chars().take(SAVED_SEARCH_NAME_CHARS).collect();
        }
        let id = s.id.trim().to_owned();
        let id = if id.is_empty() || !seen.insert(id.clone()) {
            let fresh = uuid::Uuid::new_v4().to_string();
            seen.insert(fresh.clone());
            fresh
        } else {
            id
        };
        out.push(SavedSearch { id, name, query });
        if out.len() == SAVED_SEARCHES_MAX {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use mh_core::Settings;

    use super::*;
    use crate::commands::normalize_settings;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn tag(id: &str, name: &str, color: &str) -> Tag {
        Tag {
            id: id.into(),
            name: name.into(),
            color: color.into(),
        }
    }

    fn err(s: &Store, t: Tag) -> (ErrorCode, String) {
        let e = save_tag(s, t).unwrap_err();
        (e.code, e.message)
    }

    #[test]
    fn etiket_sadelestirilip_kaydedilir() {
        let s = store();
        let saved = save_tag(&s, tag("", "  Fatura \t 2026 ", "#4c6fd8")).unwrap();
        assert!(!saved.id.is_empty(), "yeni kimlik");
        assert_eq!(saved.name, "Fatura 2026");
        assert_eq!(saved.color, "#4C6FD8", "palet yazımı");
        assert_eq!(s.tags().unwrap(), vec![saved.clone()]);

        // Yerinde güncelleme: ad ve renk değişir, sıra ve kimlik korunur.
        save_tag(&s, tag("", "Proje", TAG_COLORS[1])).unwrap();
        let mut edit = saved.clone();
        edit.name = "Faturalar".into();
        edit.color = TAG_COLORS[2].into();
        let edited = save_tag(&s, edit).unwrap();
        assert_eq!(edited.id, saved.id);
        let names: Vec<String> = s.tags().unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["Faturalar", "Proje"]);
        // Kendi adını koruyarak yeniden kaydetmek çakışma değildir.
        save_tag(&s, edited.clone()).unwrap();

        // Bilinmeyen kimlik.
        let mut ghost = edited;
        ghost.id = "yok".into();
        assert_eq!(
            err(&s, ghost),
            (
                ErrorCode::NotFound,
                "Etiket bulunamadı; silinmiş olabilir.".into()
            )
        );
    }

    #[test]
    fn gecersiz_etiketler() {
        let s = store();
        save_tag(&s, tag("", "Fatura", TAG_COLORS[0])).unwrap();
        assert_eq!(
            err(&s, tag("", " \u{200F} ", TAG_COLORS[0])),
            (ErrorCode::Invalid, "Etikete bir ad ver.".into())
        );
        assert_eq!(
            err(&s, tag("", "Yeni", "#123456")),
            (
                ErrorCode::Invalid,
                "Etiket rengi paletten seçilmeli.".into()
            )
        );
        assert_eq!(
            err(&s, tag("", "Yeni", "")),
            (
                ErrorCode::Invalid,
                "Etiket rengi paletten seçilmeli.".into()
            )
        );
        assert_eq!(
            err(&s, tag("", "FATURA", TAG_COLORS[3])),
            (ErrorCode::Invalid, "Bu adda bir etiket zaten var.".into())
        );
        // Uzun ad kısaltılır.
        let long = save_tag(&s, tag("", &"ğ".repeat(100), TAG_COLORS[0])).unwrap();
        assert_eq!(long.name.chars().count(), 40);
    }

    #[test]
    fn en_cok_50_etiket() {
        let s = store();
        for i in 0..TAGS_MAX {
            save_tag(
                &s,
                tag("", &format!("E{i}"), TAG_COLORS[i % TAG_COLORS.len()]),
            )
            .unwrap();
        }
        assert_eq!(
            err(&s, tag("", "Fazla", TAG_COLORS[0])),
            (
                ErrorCode::Invalid,
                "En çok 50 etiket olabilir. Kullanmadıklarını sil.".into()
            )
        );
        // Var olan etiket yine güncellenebilir.
        let first = s.tags().unwrap().remove(0);
        save_tag(&s, first).unwrap();
    }

    #[test]
    fn kayitli_aramalar_sadelesir() {
        let ss = |id: &str, name: &str, query: &str| SavedSearch {
            id: id.into(),
            name: name.into(),
            query: query.into(),
        };
        let out = normalize_saved_searches(vec![
            ss("k1", "  Elif'ten\tekli ", " from:elif   has:ek "),
            ss("", "", "is:okunmamış"),
            ss("k1", "Yinelenen kimlik", "tag:fatura"),
            ss("k3", "Boş sorgu", "   "),
            ss("k4", &"a".repeat(100), &"b".repeat(400)),
            ss("k5", "fatura\u{202E}fdp", "x\r\ny"),
        ]);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], ss("k1", "Elif'ten ekli", "from:elif has:ek"));
        assert!(!out[1].id.is_empty());
        assert_eq!(out[1].name, "is:okunmamış", "adsız kayda sorgu ad olur");
        assert_ne!(out[2].id, "k1", "yinelenen kimlik yenilenir");
        assert_eq!(out[2].query, "tag:fatura");
        assert_eq!(out[3].name.chars().count(), SAVED_SEARCH_NAME_CHARS);
        assert_eq!(out[3].query.chars().count(), SAVED_SEARCH_QUERY_CHARS);
        assert_eq!(out[4], ss("k5", "faturafdp", "x y"));

        // En çok 30 kayıt; ayarlar üzerinden de aynı yol.
        let many: Vec<SavedSearch> = (0..40)
            .map(|i| ss(&format!("k{i}"), &format!("A{i}"), &format!("q{i}")))
            .collect();
        let s = normalize_settings(Settings {
            saved_searches: many,
            ..Settings::default()
        });
        assert_eq!(s.saved_searches.len(), SAVED_SEARCHES_MAX);
        assert_eq!(s.saved_searches[0].id, "k0");
        assert!(normalize_settings(Settings::default())
            .saved_searches
            .is_empty());
    }
}
