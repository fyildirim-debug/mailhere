//! Şema ve sürüm geçişleri (`PRAGMA user_version`).

use rusqlite::Connection;

/// Sırayla uygulanan geçişler; indeks + 1 şema sürümüdür.
const MIGRATIONS: &[&str] = &[
    // 1: ilk şema
    r#"
    CREATE TABLE accounts (
        id            TEXT PRIMARY KEY,
        name          TEXT NOT NULL,
        email         TEXT NOT NULL,
        display_name  TEXT NOT NULL,
        color         TEXT NOT NULL,
        kind          TEXT NOT NULL,
        imap          TEXT,
        smtp          TEXT,
        jmap_url      TEXT,
        signature     TEXT,
        created_at    INTEGER NOT NULL
    );

    CREATE TABLE mailboxes (
        id           TEXT PRIMARY KEY,
        account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        path         TEXT NOT NULL,
        name         TEXT NOT NULL,
        role         TEXT NOT NULL,
        uidvalidity  INTEGER,
        uidnext      INTEGER,
        highest_uid  INTEGER NOT NULL DEFAULT 0,
        UNIQUE (account_id, path)
    );
    CREATE INDEX mailboxes_role ON mailboxes(account_id, role);

    -- Büyük `raw` sütunu en sonda: liste sorguları taşma sayfalarına inmez.
    CREATE TABLE messages (
        id                 INTEGER PRIMARY KEY,
        account_id         TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        mailbox_id         TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
        uid                INTEGER,
        uidvalidity        INTEGER,
        message_id_header  TEXT,
        in_reply_to        TEXT,
        refs               TEXT NOT NULL DEFAULT '[]',
        thread_id          TEXT NOT NULL,
        from_addr          TEXT NOT NULL,
        to_addrs           TEXT NOT NULL DEFAULT '[]',
        cc_addrs           TEXT NOT NULL DEFAULT '[]',
        reply_to           TEXT NOT NULL DEFAULT '[]',
        subject            TEXT NOT NULL DEFAULT '',
        snippet            TEXT NOT NULL DEFAULT '',
        date               INTEGER NOT NULL,
        unread             INTEGER NOT NULL DEFAULT 1,
        starred            INTEGER NOT NULL DEFAULT 0,
        has_attachments    INTEGER NOT NULL DEFAULT 0,
        category           TEXT NOT NULL DEFAULT 'people',
        snoozed_until      INTEGER,
        size               INTEGER NOT NULL DEFAULT 0,
        body_fetched       INTEGER NOT NULL DEFAULT 0,
        fts_subject        TEXT NOT NULL DEFAULT '',
        fts_sender         TEXT NOT NULL DEFAULT '',
        fts_body           TEXT NOT NULL DEFAULT '',
        raw                BLOB,
        UNIQUE (mailbox_id, uid)
    );
    CREATE INDEX messages_mailbox_date ON messages(mailbox_id, date DESC, id DESC);
    CREATE INDEX messages_date ON messages(date DESC, id DESC);
    CREATE INDEX messages_thread ON messages(account_id, thread_id);
    CREATE INDEX messages_msgid ON messages(account_id, message_id_header);
    CREATE INDEX messages_snoozed ON messages(snoozed_until) WHERE snoozed_until IS NOT NULL;

    -- Arama: harici içerikli FTS5; Türkçe için aksan katlama + ı/i katlama
    -- (katlama `fts_*` sütunlarına yazılırken Rust'ta yapılır).
    CREATE VIRTUAL TABLE messages_fts USING fts5(
        fts_subject, fts_sender, fts_body,
        content = 'messages', content_rowid = 'id',
        tokenize = 'unicode61 remove_diacritics 2'
    );
    CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
        INSERT INTO messages_fts(rowid, fts_subject, fts_sender, fts_body)
        VALUES (new.id, new.fts_subject, new.fts_sender, new.fts_body);
    END;
    CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
        INSERT INTO messages_fts(messages_fts, rowid, fts_subject, fts_sender, fts_body)
        VALUES ('delete', old.id, old.fts_subject, old.fts_sender, old.fts_body);
    END;
    CREATE TRIGGER messages_fts_au AFTER UPDATE OF fts_subject, fts_sender, fts_body ON messages BEGIN
        INSERT INTO messages_fts(messages_fts, rowid, fts_subject, fts_sender, fts_body)
        VALUES ('delete', old.id, old.fts_subject, old.fts_sender, old.fts_body);
        INSERT INTO messages_fts(rowid, fts_subject, fts_sender, fts_body)
        VALUES (new.id, new.fts_subject, new.fts_sender, new.fts_body);
    END;

    CREATE TABLE settings (
        id    INTEGER PRIMARY KEY CHECK (id = 1),
        json  TEXT NOT NULL
    );

    CREATE TABLE outbox (
        id          TEXT PRIMARY KEY,
        account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        send_at     INTEGER NOT NULL,
        payload     TEXT NOT NULL,
        attempts    INTEGER NOT NULL DEFAULT 0,
        claimed     INTEGER NOT NULL DEFAULT 0,
        last_error  TEXT,
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX outbox_due ON outbox(send_at);
    "#,
    // 2: hesabın kimlik doğrulama yolu (parola, Google, Microsoft).
    r#"
    ALTER TABLE accounts ADD COLUMN auth TEXT NOT NULL DEFAULT 'password';
    "#,
    // 3: kalıcı sunucu işlemleri kuyruğu, sunucu onayı bekleyen taşımanın
    // kaynağı, eşitlenmeyen kutular ve kişisel IMAP ad alanı.
    r#"
    -- Yerelde taşınmış ama sunucuda henüz taşınmamış posta: sunucudaki yeri
    -- (kutu, UID, UIDVALIDITY) onaya kadar burada kalır.
    ALTER TABLE messages ADD COLUMN src_mailbox_id TEXT;
    ALTER TABLE messages ADD COLUMN src_uid INTEGER;
    ALTER TABLE messages ADD COLUMN src_uidvalidity INTEGER;
    CREATE INDEX messages_src ON messages(src_mailbox_id, src_uid) WHERE src_mailbox_id IS NOT NULL;

    -- Gmail "Tüm Postalar" gibi hedef olarak kullanılıp eşitlenmeyen kutular.
    ALTER TABLE mailboxes ADD COLUMN nosync INTEGER NOT NULL DEFAULT 0;

    -- Sunucu işlemleri (bayrak, taşıma, ekleme, silme), hesap başına sırayla.
    CREATE TABLE actions (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        payload     TEXT NOT NULL,
        raw         BLOB,
        attempts    INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX actions_account ON actions(account_id, id);

    -- Yeni rol kutularının açılacağı kişisel ad alanı ("INBOX." gibi).
    CREATE TABLE imap_namespace (
        account_id  TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
        prefix      TEXT NOT NULL,
        delimiter   TEXT
    );
    "#,
    // 4: başka bir dalda tanımlı; birleştirmede gerçek geçişle değiştirilir.
    "-- yer tutucu",
    // 5: JMAP — hesap başına kullanıcı adı ve durum dizgeleri, postaların
    // sunucudaki kimlikleri. Ayrı tablolar: `messages` satırındaki büyük
    // `raw` sütunu en sonda kalsın.
    r#"
    CREATE TABLE jmap_accounts (
        account_id     TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
        username       TEXT,
        email_state    TEXT,
        mailbox_state  TEXT
    );

    CREATE TABLE jmap_messages (
        message_id  INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
        account_id  TEXT NOT NULL,
        jmap_id     TEXT NOT NULL,
        blob_id     TEXT,
        UNIQUE (account_id, jmap_id)
    );
    "#,
    // 6: klasör ağacı, gizleme ve elle atanan roller; hesap başına bildirim.
    r#"
    -- Seçilebilir en yakın üst klasörün yerel kimliği; kökteyse NULL.
    ALTER TABLE mailboxes ADD COLUMN parent_id TEXT;
    -- Kenar çubuğunda gizli (1) ya da görünür (0). Önceki sürümden kalan
    -- kutularda -1: "henüz karar verilmedi"; varsayılan bir sonraki
    -- listelemede (IMAP LIST, JMAP Mailbox/get) yazılır, sonra hiç değişmez.
    ALTER TABLE mailboxes ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0;
    UPDATE mailboxes SET hidden = -1;
    -- Kullanıcının elle atadığı rol; NULL = sunucudan bulunan (otomatik).
    -- `role` sütunu etkin roldür.
    ALTER TABLE mailboxes ADD COLUMN role_user TEXT;
    -- Bu hesabın yeni postaları için kart ve bildirim gösterilir.
    ALTER TABLE accounts ADD COLUMN notify INTEGER NOT NULL DEFAULT 1;
    -- JMAP: izlenen kutular genişledi (görünen klasörler ve Gereksiz); var
    -- olan postaları da alınsın diye sonraki eşitleme baştan yapılır.
    UPDATE jmap_accounts SET email_state = NULL, mailbox_state = NULL;
    "#,
    // 7: kurallar ve kuralla iletilen postaların kaydı (döngü ve sıklık
    // koruması). Hesaba bağlı kural hesapla birlikte silinir.
    r#"
    CREATE TABLE rules (
        id          TEXT PRIMARY KEY,
        account_id  TEXT REFERENCES accounts(id) ON DELETE CASCADE,
        position    INTEGER NOT NULL,
        json        TEXT NOT NULL
    );
    CREATE INDEX rules_position ON rules(position);

    CREATE TABLE rule_forwards (
        rule_id      TEXT NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
        message_key  TEXT NOT NULL,
        at           INTEGER NOT NULL,
        PRIMARY KEY (rule_id, message_key)
    );
    CREATE INDEX rule_forwards_at ON rule_forwards(rule_id, at);
    "#,
    // 8: sabitleme ve yerel etiketler. İkisi de yalnız bu bilgisayarda
    // tutulur, sunucuya yazılmaz. Etiketler hesaplar arası ortaktır; posta
    // silinince ya da etiket silinince bağ da gider.
    r#"
    ALTER TABLE messages ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX messages_pinned ON messages(mailbox_id, pinned) WHERE pinned = 1;

    CREATE TABLE tags (
        id        TEXT PRIMARY KEY,
        name      TEXT NOT NULL,
        color     TEXT NOT NULL,
        position  INTEGER NOT NULL
    );
    CREATE INDEX tags_position ON tags(position);

    CREATE TABLE message_tags (
        message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        tag_id      TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
        PRIMARY KEY (message_id, tag_id)
    );
    CREATE INDEX message_tags_tag ON message_tags(tag_id, message_id);
    "#,
    // 9: yanıt hatırlatmaları ve yeni gönderici eleği.
    r#"
    -- Gönderilmiş postaya kurulu yanıt hatırlatması. Posta silinince düşer.
    -- `fired_at`: süresi doldu ve posta gelen kutusuna "Yanıt bekleniyor"
    -- işaretiyle düştü; NULL ise henüz bekliyor.
    CREATE TABLE followups (
        message_id         INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
        account_id         TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        message_id_header  TEXT,
        thread_id          TEXT NOT NULL,
        due_at             INTEGER NOT NULL,
        created_at         INTEGER NOT NULL,
        fired_at           INTEGER
    );
    CREATE INDEX followups_due ON followups(due_at);

    -- Bilinen göndericiler: gönderilen postaların alıcıları ('sent') ve
    -- elekten kabul edilenler ('accepted'). Hesap başına tutulur.
    CREATE TABLE known_senders (
        account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        email       TEXT NOT NULL,
        source      TEXT NOT NULL,
        at          INTEGER NOT NULL,
        PRIMARY KEY (account_id, email)
    );

    -- Elekte bekleyen postalar: gelen kutusunda görünmez, kart açmaz.
    CREATE TABLE screened (
        message_id  INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
        account_id  TEXT NOT NULL,
        email       TEXT NOT NULL,
        at          INTEGER NOT NULL
    );
    CREATE INDEX screened_sender ON screened(account_id, email);

    -- Bilinen göndericilerin mevcut Gönderilenler'den bir kez doldurulduğu.
    CREATE TABLE screener_state (
        id          INTEGER PRIMARY KEY CHECK (id = 1),
        backfilled  INTEGER NOT NULL DEFAULT 0
    );
    "#,
    // Eski varsayılan "yalnız Kişiler" bülten ve otomatik postaları sessizce
    // geçiriyordu; varsayılan "her posta" oldu, kayıtlı eski varsayılan bir
    // kez taşınır (kullanıcı sonra yine seçebilir).
    r#"
    UPDATE settings SET json = json_set(json, '$.notify', 'all')
    WHERE json_extract(json, '$.notify') = 'people';
    "#,
];

/// Dallar ayrı geliştirilirken ana dalın ara sürümleri bazı geçişleri yer
/// tutucu olarak uyguladı (`user_version` ilerledi, tablolar oluşmadı). Bu
/// geçişler, varlığı denetlenen tablo yoksa sonradan uygulanır.
const PLACEHOLDER_REPAIRS: &[(usize, &str)] = &[(3, "actions"), (8, "tags"), (9, "followups")];

fn table_exists(conn: &Connection, name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [name],
        |r| r.get(0),
    )
}

/// Bağlantıyı ayarlar ve eksik geçişleri uygular.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    let current: usize =
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    for &(version, table) in PLACEHOLDER_REPAIRS {
        if current >= version && !table_exists(conn, table)? {
            tracing::warn!(version, "yer tutucuyla atlanmış geçiş uygulanıyor");
            let tx = conn.transaction()?;
            tx.execute_batch(MIGRATIONS[version - 1])?;
            tx.commit()?;
        }
    }
    Ok(())
}

/// Şemanın en son sürümü.
#[cfg(test)]
pub fn latest_version() -> usize {
    MIGRATIONS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eski_varsayilan_kisiler_her_postaya_tasinir() {
        let mut conn = Connection::open_in_memory().unwrap();
        let son = MIGRATIONS.len();
        for sql in &MIGRATIONS[..son - 1] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", (son - 1) as i64)
            .unwrap();
        conn.execute(
            "INSERT INTO settings (id, json) VALUES (1, '{\"notify\":\"people\",\"theme\":\"dark\"}')",
            [],
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let json: String = conn
            .query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["notify"], "all");
        assert_eq!(v["theme"], "dark", "diğer ayarlar korunur");
    }

    #[test]
    fn yer_tutucuyla_atlanan_gecis_tamamlanir() {
        // Ana dalın ara sürümü: 3. geçiş yer tutucu, sürüm en sonda.
        let mut conn = Connection::open_in_memory().unwrap();
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            conn.execute_batch(if i == 2 { "-- yer tutucu" } else { sql })
                .unwrap();
        }
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64)
            .unwrap();
        assert!(!table_exists(&conn, "actions").unwrap());

        migrate(&mut conn).unwrap();
        assert!(table_exists(&conn, "actions").unwrap());
        assert!(table_exists(&conn, "imap_namespace").unwrap());
        conn.prepare("SELECT src_mailbox_id, src_uid, src_uidvalidity FROM messages")
            .unwrap();
        conn.prepare("SELECT nosync FROM mailboxes").unwrap();
        // İkinci açılışta yeniden uygulanmaz (sütunlar zaten var).
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn hatirlatma_ve_elek_tablolari_olusur() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        for t in ["followups", "known_senders", "screened", "screener_state"] {
            assert!(table_exists(&conn, t).unwrap(), "{t}");
        }
        // 9. geçiş yer tutucuyla atlanmışsa sonradan uygulanır.
        let mut conn = Connection::open_in_memory().unwrap();
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            conn.execute_batch(if i == 8 { "-- yer tutucu" } else { sql })
                .unwrap();
        }
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64)
            .unwrap();
        assert!(!table_exists(&conn, "followups").unwrap());
        migrate(&mut conn).unwrap();
        assert!(table_exists(&conn, "followups").unwrap());
        assert!(table_exists(&conn, "screened").unwrap());
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn yer_tutucuyla_atlanan_liste_gecisi_tamamlanir() {
        // Ara sürüm 8. geçişi yer tutucu olarak uygulamış, sürüm en sonda.
        let mut conn = Connection::open_in_memory().unwrap();
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            conn.execute_batch(if i == 7 { "-- yer tutucu" } else { sql })
                .unwrap();
        }
        conn.pragma_update(None, "user_version", MIGRATIONS.len() as i64)
            .unwrap();
        assert!(!table_exists(&conn, "tags").unwrap());
        migrate(&mut conn).unwrap();
        assert!(table_exists(&conn, "tags").unwrap());
        assert!(table_exists(&conn, "message_tags").unwrap());
        conn.prepare("SELECT pinned FROM messages").unwrap();
        // İkinci açılışta yeniden uygulanmaz (sütun zaten var).
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn klasor_agaci_gecisi_eski_kutulari_karasiz_birakir() {
        // 5. sürümde bir hesap ve kutu; 6. geçiş sonradan uygulanır.
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5i64).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, display_name, color, kind, created_at)
             VALUES ('a', 'İş', 'a@ornek.com', 'A', '#4C6FD8', 'imap', 1);
             INSERT INTO mailboxes (id, account_id, path, name, role)
             VALUES ('a:Projeler', 'a', 'Projeler', 'Projeler', 'other');
             INSERT INTO jmap_accounts (account_id, username, email_state, mailbox_state)
             VALUES ('a', 'furkan', 'e9', 'm3');",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let (parent, hidden, role_user): (Option<String>, i64, Option<String>) = conn
            .query_row(
                "SELECT parent_id, hidden, role_user FROM mailboxes WHERE id = 'a:Projeler'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((parent, hidden, role_user), (None, -1, None));
        let notify: bool = conn
            .query_row("SELECT notify FROM accounts WHERE id = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(notify, "var olan hesaplarda bildirim açık");
        let jmap: (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT username, email_state, mailbox_state FROM jmap_accounts WHERE account_id = 'a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            jmap,
            (Some("furkan".into()), None, None),
            "JMAP baştan eşitlenir"
        );
        // Yeni kutu karar verilmiş (görünür) olarak eklenir.
        conn.execute(
            "INSERT INTO mailboxes (id, account_id, path, name, role) VALUES ('a:X', 'a', 'X', 'X', 'other')",
            [],
        )
        .unwrap();
        let hidden: i64 = conn
            .query_row("SELECT hidden FROM mailboxes WHERE id = 'a:X'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(hidden, 0);
    }

    #[test]
    fn sabitleme_ve_etiket_gecisi_eski_postalari_korur() {
        // 7. sürümde bir posta; 8. geçiş sonradan uygulanır.
        let mut conn = Connection::open_in_memory().unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7i64).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, display_name, color, kind, created_at)
             VALUES ('a', 'İş', 'a@ornek.com', 'A', '#4C6FD8', 'imap', 1);
             INSERT INTO mailboxes (id, account_id, path, name, role)
             VALUES ('a:INBOX', 'a', 'INBOX', 'INBOX', 'inbox');
             INSERT INTO messages (account_id, mailbox_id, thread_id, from_addr, date)
             VALUES ('a', 'a:INBOX', 't', '{\"name\":null,\"email\":\"x@y.z\"}', 5);",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        let pinned: i64 = conn
            .query_row("SELECT pinned FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(pinned, 0, "eski postalar sabitlenmemiş");
        assert!(table_exists(&conn, "tags").unwrap());
        assert!(table_exists(&conn, "message_tags").unwrap());
        // Etiket bağı posta silinince gider.
        conn.execute_batch(
            "INSERT INTO tags (id, name, color, position) VALUES ('e1', 'Fatura', '#4C6FD8', 0);
             INSERT INTO message_tags (message_id, tag_id) SELECT id, 'e1' FROM messages;
             DELETE FROM messages;",
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM message_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
