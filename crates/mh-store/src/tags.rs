//! Sabitleme, yerel etiketler ve göndericiye göre toplu seçim.
//!
//! Sabitleme ve etiketler yalnız bu bilgisayarda tutulur; sunucuya hiçbir
//! bayrak ya da klasör yazılmaz. Etiketler hesaplar arası ortaktır; posta ya
//! da etiket silinince bağ da silinir (şema 8, `ON DELETE CASCADE`).

use mh_core::{AppError, AppResult, ErrorCode, MessageId, Tag, View, TAG_NAME_CHARS};
use mh_mime::{clean_line, collapse_ws};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

use crate::codec::from_json;
use crate::search::fold_loose;
use crate::Store;

/// Bu görünümde sabitlenenler listenin başında mı gösterilir?
pub(crate) fn pinned_first(view: &View) -> bool {
    matches!(view, View::Inbox | View::Mailbox { .. })
}

/// Etiket adını sadeleştirir: denetim ve yön karakterleri atılır, boşluklar
/// teke iner, en çok `TAG_NAME_CHARS` karakter.
pub fn normalize_tag_name(name: &str) -> String {
    let line = collapse_ws(&clean_line(name));
    let cut: String = line.trim().chars().take(TAG_NAME_CHARS).collect();
    cut.trim_end().to_owned()
}

/// Göndericiye göre seçimin kapsamı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderScope {
    /// Hesapların gelen kutuları (ertelenenler dahil).
    Inbox,
    /// Tek bir klasör.
    Mailbox(String),
    /// Bir etiketi taşıyan postalar (çöp ve istenmeyen hariç).
    Tag(String),
}

impl SenderScope {
    /// Arayüzün gösterdiği görünümden kapsam: klasör ve etiket görünümleri
    /// kendileri, diğer her şey gelen kutusu.
    pub fn of(view: Option<&View>) -> Self {
        match view {
            Some(View::Mailbox { id }) => SenderScope::Mailbox(id.clone()),
            Some(View::Tag { id }) => SenderScope::Tag(id.clone()),
            _ => SenderScope::Inbox,
        }
    }
}

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn ids_params(ids: &[MessageId]) -> Vec<Value> {
    ids.iter().map(|i| Value::Integer(*i)).collect()
}

fn row_to_tag(r: &Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
    })
}

fn tag_gone() -> AppError {
    AppError::new(ErrorCode::NotFound, "Etiket bulunamadı; silinmiş olabilir.")
}

impl Store {
    /// Postaları sabitler ya da sabitlemeyi kaldırır; etkilenen kutuları döner.
    pub fn set_pinned(&self, ids: &[MessageId], pinned: bool) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let mut p = vec![Value::Integer(pinned as i64)];
            p.extend(ids_params(ids));
            c.execute(
                &format!(
                    "UPDATE messages SET pinned = ? WHERE id IN ({})",
                    in_list(ids.len())
                ),
                params_from_iter(p),
            )
        })?;
        self.mailboxes_of(ids)
    }

    /// Verilen postaların bulunduğu kutular (sıralı, tekrarsız).
    fn mailboxes_of(&self, ids: &[MessageId]) -> AppResult<Vec<String>> {
        let mut out: Vec<String> = self
            .locations(ids)?
            .into_iter()
            .map(|l| l.mailbox_id)
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    // ------------------------------------------------------------- etiketler

    /// Tüm etiketler, kullanıcı sırasıyla.
    pub fn tags(&self) -> AppResult<Vec<Tag>> {
        self.with(|c| {
            let mut st =
                c.prepare_cached("SELECT id, name, color FROM tags ORDER BY position, id")?;
            let rows = st.query_map([], row_to_tag)?;
            rows.collect()
        })
    }

    pub fn tag(&self, id: &str) -> AppResult<Option<Tag>> {
        self.with(|c| {
            c.query_row(
                "SELECT id, name, color FROM tags WHERE id = ?1",
                [id],
                row_to_tag,
            )
            .optional()
        })
    }

    pub fn tag_count(&self) -> AppResult<usize> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
        })
    }

    /// Bu adda (büyük/küçük harf ve Türkçe harf farkı gözetmeden) başka bir
    /// etiket var mı? `except` kimliği sayılmaz (yeniden adlandırma).
    pub fn tag_name_taken(&self, name: &str, except: Option<&str>) -> AppResult<bool> {
        let wanted = fold_loose(name);
        Ok(self
            .tags()?
            .iter()
            .any(|t| except != Some(t.id.as_str()) && fold_loose(&t.name) == wanted))
    }

    /// Yeni etiketi sona ekler.
    pub fn tag_insert(&self, tag: &Tag) -> AppResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO tags (id, name, color, position)
                 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position) + 1, 0) FROM tags))",
                params![tag.id, tag.name, tag.color],
            )?;
            Ok(())
        })
    }

    /// Var olan etiketin adını ve rengini yazar; yoksa `false`.
    pub fn tag_update(&self, tag: &Tag) -> AppResult<bool> {
        self.with(|c| {
            Ok(c.execute(
                "UPDATE tags SET name = ?2, color = ?3 WHERE id = ?1",
                params![tag.id, tag.name, tag.color],
            )? > 0)
        })
    }

    /// Etiketi siler; bağlı postaların kutularını döner (liste yenilensin).
    /// Etiket yoksa boş liste.
    pub fn tag_delete(&self, id: &str) -> AppResult<Vec<String>> {
        let ids: Vec<MessageId> = self.with(|c| {
            let mut st =
                c.prepare_cached("SELECT message_id FROM message_tags WHERE tag_id = ?1")?;
            let rows = st.query_map([id], |r| r.get::<_, MessageId>(0))?;
            rows.collect()
        })?;
        let touched = self.mailboxes_of(&ids)?;
        self.with(|c| {
            c.execute("DELETE FROM tags WHERE id = ?1", [id])?;
            Ok(())
        })?;
        Ok(touched)
    }

    /// Etiketi postalara ekler ya da kaldırır; etkilenen kutuları döner.
    /// Etiket yoksa hata.
    pub fn set_message_tag(
        &self,
        ids: &[MessageId],
        tag_id: &str,
        on: bool,
    ) -> AppResult<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if self.tag(tag_id)?.is_none() {
            return Err(tag_gone());
        }
        self.with_tx(|tx| {
            if on {
                let mut st = tx.prepare_cached(
                    "INSERT OR IGNORE INTO message_tags (message_id, tag_id)
                     SELECT id, ?2 FROM messages WHERE id = ?1",
                )?;
                for id in ids {
                    st.execute(params![id, tag_id])?;
                }
            } else {
                let mut p = vec![Value::Text(tag_id.into())];
                p.extend(ids_params(ids));
                tx.execute(
                    &format!(
                        "DELETE FROM message_tags WHERE tag_id = ? AND message_id IN ({})",
                        in_list(ids.len())
                    ),
                    params_from_iter(p),
                )?;
            }
            Ok(())
        })?;
        self.mailboxes_of(ids)
    }

    // ------------------------------------------------------- göndericiye göre

    /// Kapsamdaki, `email` adresinden gelen postalar; yeniden eskiye. Adres
    /// büyük/küçük harf farkı gözetmeden tam eşleşir. `account_id` verilirse
    /// yalnız o hesap.
    pub fn messages_by_sender(
        &self,
        account_id: Option<&str>,
        email: &str,
        scope: &SenderScope,
    ) -> AppResult<Vec<MessageId>> {
        let email = email.trim().to_ascii_lowercase();
        if email.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = String::from(
            "SELECT m.id, m.from_addr FROM messages m JOIN mailboxes b ON b.id = m.mailbox_id
             WHERE instr(mh_addrs(m.from_addr), ?) > 0",
        );
        let mut p: Vec<Value> = vec![Value::Text(fold_loose(&email))];
        match scope {
            SenderScope::Inbox => sql += " AND b.role = 'inbox'",
            SenderScope::Mailbox(id) => {
                sql += " AND m.mailbox_id = ?";
                p.push(Value::Text(id.clone()));
            }
            SenderScope::Tag(id) => {
                sql += " AND EXISTS(SELECT 1 FROM message_tags mt WHERE mt.message_id = m.id AND mt.tag_id = ?)
                         AND b.role NOT IN ('trash', 'junk')";
                p.push(Value::Text(id.clone()));
            }
        }
        if let Some(a) = account_id {
            sql += " AND m.account_id = ?";
            p.push(Value::Text(a.into()));
        }
        sql += " ORDER BY m.date DESC, m.id DESC";
        let rows: Vec<(MessageId, String)> = self.with(|c| {
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(p), |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })?;
        // `mh_addrs` ön süzgeçtir (ad da içerir); adres burada tam karşılaştırılır.
        Ok(rows
            .into_iter()
            .filter(|(_, json)| {
                from_json::<mh_core::Address>(1, json)
                    .map(|a| a.email.trim().eq_ignore_ascii_case(&email))
                    .unwrap_or(false)
            })
            .map(|(id, _)| id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use mh_core::{Address, ListQuery, MailboxRole};

    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::{account, store_with_account};

    fn tag(id: &str, name: &str) -> Tag {
        Tag {
            id: id.into(),
            name: name.into(),
            color: "#4C6FD8".into(),
        }
    }

    fn setup() -> Store {
        let s = store_with_account("a");
        s.insert_account(&account("b")).unwrap();
        for acc in ["a", "b"] {
            for (path, role) in [
                ("INBOX", MailboxRole::Inbox),
                ("Archive", MailboxRole::Archive),
                ("Trash", MailboxRole::Trash),
                ("Projeler", MailboxRole::Other),
            ] {
                s.upsert_mailbox(acc, path, path, role, None, false)
                    .unwrap();
            }
        }
        s
    }

    fn query(view: View, limit: u32) -> ListQuery {
        ListQuery {
            view,
            account_id: None,
            category: None,
            only: None,
            cursor: None,
            limit,
        }
    }

    fn subjects(s: &Store, q: &ListQuery) -> Vec<String> {
        s.list(q, 1_000)
            .unwrap()
            .items
            .into_iter()
            .map(|m| m.subject)
            .collect()
    }

    #[test]
    fn etiket_adi_sadelesir() {
        assert_eq!(normalize_tag_name("  Fatura \t  2026 "), "Fatura 2026");
        assert_eq!(normalize_tag_name("a\u{202E}b\r\nc"), "ab c");
        assert_eq!(normalize_tag_name(&"ş".repeat(80)).chars().count(), 40);
        assert_eq!(normalize_tag_name(" \u{200F} "), "");
    }

    #[test]
    fn sabitlenenler_gelen_kutusunda_ve_klasorde_ustte() {
        let s = setup();
        let ids: Vec<MessageId> = (0..5)
            .map(|i| {
                s.upsert_message(&msg("a", "a:INBOX", &format!("K{i}"), 100 + i))
                    .unwrap()
                    .id
            })
            .collect();
        // En eski iki posta sabitlenir: listenin başına, kendi aralarında tarihe göre.
        let touched = s.set_pinned(&[ids[0], ids[1]], true).unwrap();
        assert_eq!(touched, vec!["a:INBOX".to_string()]);
        assert_eq!(
            subjects(&s, &query(View::Inbox, 50)),
            vec!["K1", "K0", "K4", "K3", "K2"]
        );
        let page = s.list(&query(View::Inbox, 50), 1_000).unwrap();
        assert!(page.items[0].pinned && page.items[1].pinned && !page.items[2].pinned);
        assert_eq!(
            subjects(
                &s,
                &query(
                    View::Mailbox {
                        id: "a:INBOX".into()
                    },
                    50
                )
            ),
            vec!["K1", "K0", "K4", "K3", "K2"]
        );
        // Yıldızlılar gibi görünümlerde sabitleme sırayı değiştirmez.
        s.set_flags(&ids, None, Some(true)).unwrap();
        assert_eq!(
            subjects(&s, &query(View::Starred, 50)),
            vec!["K4", "K3", "K2", "K1", "K0"]
        );
        // Kaldırma.
        s.set_pinned(&[ids[0]], false).unwrap();
        assert_eq!(
            subjects(&s, &query(View::Inbox, 50)),
            vec!["K1", "K4", "K3", "K2", "K0"]
        );
        assert!(s.set_pinned(&[], true).unwrap().is_empty());
    }

    #[test]
    fn sabitli_listede_imlecli_sayfalama_tekrarsiz() {
        let s = setup();
        let ids: Vec<MessageId> = (0..7)
            .map(|i| {
                s.upsert_message(&msg("a", "a:INBOX", &format!("K{i}"), 100 + i))
                    .unwrap()
                    .id
            })
            .collect();
        // Sabitlenenler sayfa sınırında bölünsün diye üç tane.
        s.set_pinned(&[ids[0], ids[2], ids[4]], true).unwrap();
        let mut q = query(View::Inbox, 2);
        let mut seen = Vec::new();
        let mut pages = 0;
        loop {
            let page = s.list(&q, 1_000).unwrap();
            pages += 1;
            seen.extend(page.items.iter().map(|m| m.subject.clone()));
            match page.next_cursor {
                Some(c) => {
                    assert_eq!(c.matches(':').count(), 2, "sabitli imleç üç parçalı: {c}");
                    q.cursor = Some(c);
                }
                None => break,
            }
        }
        assert_eq!(pages, 4);
        assert_eq!(seen, vec!["K4", "K2", "K0", "K6", "K5", "K3", "K1"]);
        // Bozuk üçüncü parça reddedilir.
        q.cursor = Some("100:1:x".into());
        assert_eq!(s.list(&q, 1_000).unwrap_err().code, ErrorCode::Invalid);
        q.cursor = Some("100:1:1:9".into());
        assert_eq!(s.list(&q, 1_000).unwrap_err().code, ErrorCode::Invalid);
    }

    #[test]
    fn etiket_ekle_oku_guncelle_sil() {
        let s = setup();
        s.tag_insert(&tag("e1", "Fatura")).unwrap();
        s.tag_insert(&tag("e2", "Proje")).unwrap();
        let names = |s: &Store| {
            s.tags()
                .unwrap()
                .into_iter()
                .map(|t| t.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&s), vec!["Fatura", "Proje"]);
        assert_eq!(s.tag_count().unwrap(), 2);
        assert_eq!(s.tag("e2").unwrap().unwrap().name, "Proje");
        assert!(s.tag("yok").unwrap().is_none());
        // Ad çakışması Türkçe harf ve büyük/küçük harf duyarsız.
        assert!(s.tag_name_taken("FATURA", None).unwrap());
        assert!(s.tag_name_taken("fatura", Some("e2")).unwrap());
        assert!(!s.tag_name_taken("fatura", Some("e1")).unwrap());
        assert!(!s.tag_name_taken("Kişisel", None).unwrap());
        // Güncelleme sırayı korur.
        let mut e1 = tag("e1", "Faturalar");
        e1.color = "#2E9A61".into();
        assert!(s.tag_update(&e1).unwrap());
        assert!(!s.tag_update(&tag("yok", "x")).unwrap());
        assert_eq!(names(&s), vec!["Faturalar", "Proje"]);
        assert_eq!(s.tag("e1").unwrap().unwrap().color, "#2E9A61");
    }

    #[test]
    fn posta_etiketleri_ozet_gorunum_ve_silme() {
        let s = setup();
        s.tag_insert(&tag("e1", "Fatura")).unwrap();
        s.tag_insert(&tag("e2", "Proje")).unwrap();
        let m1 = s
            .upsert_message(&msg("a", "a:INBOX", "Ekim faturası", 10))
            .unwrap()
            .id;
        let m2 = s
            .upsert_message(&msg("a", "a:Projeler", "Yol haritası", 11))
            .unwrap()
            .id;
        let cop = s
            .upsert_message(&msg("a", "a:Trash", "Silinen fatura", 12))
            .unwrap()
            .id;
        let touched = s.set_message_tag(&[m1, m2, cop], "e1", true).unwrap();
        assert_eq!(
            touched,
            vec!["a:INBOX".to_string(), "a:Projeler".into(), "a:Trash".into()]
        );
        // Yineleme sorun değil; ikinci etiket sıraya göre gelir.
        s.set_message_tag(&[m1], "e1", true).unwrap();
        s.set_message_tag(&[m1], "e2", true).unwrap();
        assert_eq!(
            s.summary(m1).unwrap().unwrap().tags,
            vec!["e1".to_string(), "e2".into()]
        );
        assert_eq!(s.summary(m2).unwrap().unwrap().tags, vec!["e1".to_string()]);
        // Etiket görünümü: çöp hariç, yeniden eskiye.
        assert_eq!(
            subjects(&s, &query(View::Tag { id: "e1".into() }, 50)),
            vec!["Yol haritası", "Ekim faturası"]
        );
        assert_eq!(
            subjects(&s, &query(View::Tag { id: "e2".into() }, 50)),
            vec!["Ekim faturası"]
        );
        // Kaldırma.
        s.set_message_tag(&[m1], "e1", false).unwrap();
        assert_eq!(s.summary(m1).unwrap().unwrap().tags, vec!["e2".to_string()]);
        // Bilinmeyen etiket.
        let e = s.set_message_tag(&[m1], "yok", true).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(e.message, "Etiket bulunamadı; silinmiş olabilir.");
        assert!(s.set_message_tag(&[], "e1", true).unwrap().is_empty());
        // Etiket silinince bağlar gider; etkilenen kutular döner.
        let touched = s.tag_delete("e1").unwrap();
        assert_eq!(touched, vec!["a:Projeler".to_string(), "a:Trash".into()]);
        assert!(s.summary(m2).unwrap().unwrap().tags.is_empty());
        assert!(s.tag_delete("e1").unwrap().is_empty());
        assert!(subjects(&s, &query(View::Tag { id: "e1".into() }, 50)).is_empty());
        // Posta silinince bağ gider.
        s.delete_messages(&[m1]).unwrap();
        let n: i64 = s
            .with(|c| c.query_row("SELECT COUNT(*) FROM message_tags", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn gondericiye_gore_postalar() {
        let s = setup();
        let from = |email: &str, mailbox: &str, subject: &str, date| {
            let mut m = msg("a", mailbox, subject, date);
            m.from = Address::new(Some("Bulut Fatura"), email);
            m
        };
        let eski = s
            .upsert_message(&from("Fatura@Bulut.com", "a:INBOX", "Eylül", 10))
            .unwrap()
            .id;
        let yeni = s
            .upsert_message(&from("fatura@bulut.com", "a:INBOX", "Ekim", 20))
            .unwrap()
            .id;
        let klasor = s
            .upsert_message(&from("fatura@bulut.com", "a:Projeler", "Klasörde", 30))
            .unwrap()
            .id;
        // Ad adresi içeriyor ama adres farklı: eşleşmez.
        let mut sahte = msg("a", "a:INBOX", "Sahte", 40);
        sahte.from = Address::new(Some("fatura@bulut.com"), "x@kotu.example");
        s.upsert_message(&sahte).unwrap();
        // Adresin bir parçası: eşleşmez.
        let mut parca = msg("a", "a:INBOX", "Parça", 41);
        parca.from = Address::new(None, "xfatura@bulut.com");
        s.upsert_message(&parca).unwrap();
        // Başka hesapta aynı gönderici.
        let mut b = msg("b", "b:INBOX", "B hesabı", 50);
        b.from = Address::new(None, "fatura@bulut.com");
        let b = s.upsert_message(&b).unwrap().id;

        assert_eq!(
            s.messages_by_sender(None, " FATURA@bulut.com ", &SenderScope::Inbox)
                .unwrap(),
            vec![b, yeni, eski],
            "yeniden eskiye, tüm hesaplar"
        );
        assert_eq!(
            s.messages_by_sender(Some("a"), "fatura@bulut.com", &SenderScope::Inbox)
                .unwrap(),
            vec![yeni, eski]
        );
        assert_eq!(
            s.messages_by_sender(
                None,
                "fatura@bulut.com",
                &SenderScope::Mailbox("a:Projeler".into())
            )
            .unwrap(),
            vec![klasor]
        );
        s.tag_insert(&tag("e1", "Fatura")).unwrap();
        s.set_message_tag(&[eski, klasor], "e1", true).unwrap();
        assert_eq!(
            s.messages_by_sender(None, "fatura@bulut.com", &SenderScope::Tag("e1".into()))
                .unwrap(),
            vec![klasor, eski]
        );
        assert!(s
            .messages_by_sender(None, "", &SenderScope::Inbox)
            .unwrap()
            .is_empty());
        assert!(s
            .messages_by_sender(None, "yok@bulut.com", &SenderScope::Inbox)
            .unwrap()
            .is_empty());
        // Görünümden kapsam.
        assert_eq!(SenderScope::of(None), SenderScope::Inbox);
        assert_eq!(SenderScope::of(Some(&View::Starred)), SenderScope::Inbox);
        assert_eq!(
            SenderScope::of(Some(&View::Mailbox { id: "a:X".into() })),
            SenderScope::Mailbox("a:X".into())
        );
        assert_eq!(
            SenderScope::of(Some(&View::Tag { id: "e1".into() })),
            SenderScope::Tag("e1".into())
        );
    }
}
