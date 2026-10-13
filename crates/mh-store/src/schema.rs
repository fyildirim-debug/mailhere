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
];

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
    Ok(())
}

/// Şemanın en son sürümü.
#[cfg(test)]
pub fn latest_version() -> usize {
    MIGRATIONS.len()
}
