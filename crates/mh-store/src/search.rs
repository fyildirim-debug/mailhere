//! Tam metin arama (FTS5).

use mh_core::{AppResult, MessageSummary};
use mh_mime::fold_tr;
use rusqlite::params;

use crate::messages::{row_to_summary, SUMMARY_COLS};
use crate::Store;

/// Kullanıcı girdisini güvenli bir FTS5 sorgusuna çevirir: harf/rakam
/// dışındaki her şey ayırıcıdır, her sözcük önek olarak aranır ve tümü
/// birlikte (VE) eşleşmelidir. Boş girdi `None` döner.
pub fn fts_query(input: &str) -> Option<String> {
    let folded = fold_tr(input);
    let terms: Vec<String> = folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(12)
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

impl Store {
    /// Konu, gönderen ve gövdede arar; en alakalı ve yeni önce. Çöp ve
    /// istenmeyen kutuları hariçtir.
    pub fn search(&self, q: &str, limit: u32) -> AppResult<Vec<MessageSummary>> {
        let Some(query) = fts_query(q) else {
            return Ok(Vec::new());
        };
        let limit = limit.clamp(1, 200) as i64;
        self.with(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {SUMMARY_COLS} FROM messages_fts f
                 JOIN messages m ON m.id = f.rowid
                 JOIN mailboxes b ON b.id = m.mailbox_id
                 WHERE messages_fts MATCH ?1 AND b.role NOT IN ('trash', 'junk')
                 ORDER BY bm25(messages_fts, 4.0, 2.0, 1.0), m.date DESC
                 LIMIT ?2"
            ))?;
            let rows = st.query_map(params![query, limit], row_to_summary)?;
            rows.collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::tests::msg;
    use crate::testutil::store_with_account;
    use mh_core::{Address, MailboxRole};

    #[test]
    fn sorgu_temizlenir() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("\"* - ()"), None);
        assert_eq!(fts_query("x OR y"), Some("\"x\"* \"or\"* \"y\"*".into()));
        assert_eq!(
            fts_query("bütçe NEAR(x)"),
            Some("\"bütçe\"* \"near\"* \"x\"*".into())
        );
        assert_eq!(fts_query("İSTANBUL"), Some("\"istanbul\"*".into()));
        assert_eq!(fts_query("a\"b"), Some("\"a\"* \"b\"*".into()));
    }

    #[test]
    fn turkce_dostu_arama() {
        let s = store_with_account("a");
        s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox)
            .unwrap();
        s.upsert_mailbox("a", "Trash", "Trash", MailboxRole::Trash)
            .unwrap();
        let mut m1 = msg("a", "a:INBOX", "Q4 bütçe taslağı", 10);
        m1.body_text = Some("Pazarlama kaleminde düşüş var.".into());
        s.upsert_message(&m1).unwrap();
        let mut m2 = msg("a", "a:INBOX", "Cumartesi kahvaltı?", 20);
        m2.from = Address::new(Some("Ayşe Demir"), "ayse.demir@eposta.com");
        m2.body_text = Some("Moda'daki yeni yere gidelim mi? IŞIK güzel.".into());
        s.upsert_message(&m2).unwrap();
        let mut gone = msg("a", "a:Trash", "Bütçe eski", 30);
        gone.body_text = Some("silindi".into());
        s.upsert_message(&gone).unwrap();

        let subj = |q: &str| {
            s.search(q, 10)
                .unwrap()
                .into_iter()
                .map(|m| m.subject)
                .collect::<Vec<_>>()
        };
        assert_eq!(subj("bütçe"), vec!["Q4 bütçe taslağı"]);
        assert_eq!(subj("butce"), vec!["Q4 bütçe taslağı"], "aksansız yazım");
        assert_eq!(subj("BÜTÇE"), vec!["Q4 bütçe taslağı"]);
        assert_eq!(subj("kahvalti"), vec!["Cumartesi kahvaltı?"], "ı → i");
        assert_eq!(subj("ışık"), vec!["Cumartesi kahvaltı?"]);
        assert_eq!(subj("ayse"), vec!["Cumartesi kahvaltı?"], "gönderen adı");
        assert_eq!(subj("pazar"), vec!["Q4 bütçe taslağı"], "önek");
        assert_eq!(subj("moda gidelim"), vec!["Cumartesi kahvaltı?"]);
        assert!(subj("moda bütçe").is_empty());
        assert!(subj("\")(*").is_empty());
        assert!(subj("zzz").is_empty());
    }
}
