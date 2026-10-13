//! Diskte şifreleme: düz metin sızıntısı, yanlış anahtar, şifresiz
//! veritabanının dönüşümü, yarıda kalan dönüşüm, anahtar kasası, taşıma.

use std::fs;
use std::path::{Path, PathBuf};

use mh_core::{ErrorCode, MailboxRole};
use rusqlite::Connection;

use crate::encryption::{self, sidecar, OLD_SUFFIX, TMP_SUFFIX};
use crate::key::tests::TestVault;
use crate::messages::tests::msg;
use crate::testutil::{account, TempDir};
use crate::{schema, DbFile, DbKey, Store};

const SUBJECT: &str = "zqxkonumarker7731";
const BODY: &str = "zqxgovdemarker5512";
const RAW: &str = "zqxhammarker9043";
const MAGIC: &[u8] = b"SQLite format 3";

/// Hesap, kutu ve işaretli bir posta yazar.
fn fill(s: &Store) {
    s.insert_account(&account("a")).unwrap();
    s.upsert_mailbox("a", "INBOX", "INBOX", MailboxRole::Inbox)
        .unwrap();
    let mut m = msg("a", "a:INBOX", SUBJECT, 10);
    m.body_text = Some(format!("Bütçe {BODY} ekte."));
    m.raw = Some(format!("Subject: {SUBJECT}\r\n\r\n{RAW}\r\n").into_bytes());
    s.upsert_message(&m).unwrap();
}

/// Önceki sürümlerin yazdığı gibi şifresiz, WAL kipinde veritabanı.
fn plain_store(path: &Path) -> Store {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    Store::init(conn).unwrap()
}

/// Klasördeki dosyaların baytları (veritabanı, WAL, yan dosyalar). `-shm`
/// yalnız WAL dizinidir (içerik taşımaz) ve Windows'ta açıkken kilitlidir.
fn all_bytes(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file() && !p.to_string_lossy().ends_with("-shm"))
        .map(|p| {
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn assert_no_plaintext(dir: &Path) {
    let files = all_bytes(dir);
    assert!(!files.is_empty());
    for (p, b) in files {
        for marker in [SUBJECT.as_bytes(), BODY.as_bytes(), RAW.as_bytes(), MAGIC] {
            assert!(
                !contains(&b, marker),
                "{} içinde düz metin: {}",
                p.display(),
                String::from_utf8_lossy(marker)
            );
        }
    }
}

fn found(s: &Store) -> Vec<String> {
    s.search(SUBJECT, 10)
        .unwrap()
        .into_iter()
        .map(|m| m.subject)
        .collect()
}

#[test]
fn sifreli_dosyada_duz_metin_yok() {
    let dir = TempDir::new("duzmetin");
    let path = dir.path().join("t.db");
    let key = DbKey::generate().unwrap();
    {
        let s = Store::open(&path, &key).unwrap();
        fill(&s);
        // Açıkken WAL'da da düz metin olmamalı.
        assert_no_plaintext(dir.path());
    }
    assert_eq!(encryption::probe(&path).unwrap(), DbFile::Encrypted);
    assert_no_plaintext(dir.path());

    let s = Store::open(&path, &key).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    assert_eq!(s.schema_version().unwrap(), schema::latest_version() as i64);
}

#[test]
fn karsilastirma_duz_veritabaninda_isaret_gorunur() {
    // Yukarıdaki denetimin anlamlı olduğunu gösterir: şifresiz dosyada
    // aynı işaretler bulunur.
    let dir = TempDir::new("kontrol");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.starts_with(MAGIC));
    assert!(contains(&bytes, SUBJECT.as_bytes()));
    assert!(contains(&bytes, RAW.as_bytes()));
}

#[test]
fn yanlis_anahtar_hata_verir_dosyaya_dokunmaz() {
    let dir = TempDir::new("yanlis");
    let path = dir.path().join("t.db");
    let key = DbKey::generate().unwrap();
    fill(&Store::open(&path, &key).unwrap());
    let before = fs::read(&path).unwrap();

    let e = Store::open(&path, &DbKey::generate().unwrap())
        .err()
        .expect("yanlış anahtarla açılmamalı");
    assert_eq!(e, encryption::undecryptable_error(&path));
    assert_eq!(e.code, ErrorCode::Storage);
    assert!(e.message.contains("silinmedi"));
    assert_eq!(fs::read(&path).unwrap(), before, "dosya değişmemeli");

    let s = Store::open(&path, &key).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
}

#[test]
fn bozuk_dosya_hata_verir() {
    let dir = TempDir::new("bozuk");
    let path = dir.path().join("t.db");
    fs::write(&path, vec![0x5a; 8192]).unwrap();
    let e = Store::open(&path, &DbKey::generate().unwrap())
        .err()
        .unwrap();
    assert_eq!(e.code, ErrorCode::Storage);
    assert_eq!(fs::read(&path).unwrap(), vec![0x5a; 8192]);
}

#[test]
fn duz_veritabani_sifrelenir_veri_ve_arama_korunur() {
    let dir = TempDir::new("donusum");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert_eq!(encryption::probe(&path).unwrap(), DbFile::Plain);

    let key = DbKey::generate().unwrap();
    {
        let s = Store::open(&path, &key).unwrap();
        assert_eq!(found(&s), vec![SUBJECT]);
        assert_eq!(s.search(BODY, 10).unwrap().len(), 1, "gövde araması");
        assert_eq!(s.accounts().unwrap().len(), 1);
        assert_eq!(s.schema_version().unwrap(), schema::latest_version() as i64);
    }
    assert_eq!(encryption::probe(&path).unwrap(), DbFile::Encrypted);
    assert!(!sidecar(&path, TMP_SUFFIX).exists());
    assert!(!sidecar(&path, OLD_SUFFIX).exists());
    assert_no_plaintext(dir.path());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn donusum_user_version_degerini_korur() {
    let dir = TempDir::new("surum");
    let path = dir.path().join("t.db");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE t (x TEXT);
             INSERT INTO t VALUES ('bir'), ('iki');
             PRAGMA user_version = 4242;",
        )
        .unwrap();
    }
    let key = DbKey::generate().unwrap();
    encryption::prepare(&path, &key).unwrap();
    let c = encryption::open_keyed(&path, &key).unwrap();
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 4242);
    let n: i64 = c
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

/// Açık bir bağlantının WAL'ı (henüz ana dosyaya geçmemiş veri) ile birlikte
/// kopyalanmış veritabanı: çökme sonrası diskte kalan duruma benzer.
fn plain_with_pending_wal(dir: &Path) -> PathBuf {
    let src_dir = dir.join("kaynak");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join("t.db");
    let s = plain_store(&src);
    s.with(|c| c.pragma_update(None, "wal_autocheckpoint", 0))
        .unwrap();
    fill(&s);
    let copy = dir.join("t.db");
    // `fs::copy` değil: Windows'ta açık dosyayı paylaşımlı okumak için.
    fs::write(&copy, fs::read(&src).unwrap()).unwrap();
    fs::write(
        sidecar(&copy, "-wal"),
        fs::read(sidecar(&src, "-wal")).unwrap(),
    )
    .unwrap();
    drop(s);
    fs::remove_dir_all(&src_dir).unwrap();
    copy
}

#[test]
fn bosaltilmamis_wal_donusume_dahil() {
    let dir = TempDir::new("wal");
    let path = plain_with_pending_wal(dir.path());
    assert!(fs::metadata(sidecar(&path, "-wal")).unwrap().len() > 0);
    let key = DbKey::generate().unwrap();
    let s = Store::open(&path, &key).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    drop(s);
    assert_no_plaintext(dir.path());
}

#[test]
fn donusum_basarisizsa_duz_dosya_oldugu_gibi_kalir() {
    let dir = TempDir::new("basarisiz");
    let path = dir.path().join("t.db");
    let mut broken = MAGIC.to_vec();
    broken.extend([0u8, 0xff].repeat(2048));
    fs::write(&path, &broken).unwrap();
    let e = Store::open(&path, &DbKey::generate().unwrap())
        .err()
        .unwrap();
    assert_eq!(e.code, ErrorCode::Storage);
    assert!(e.message.contains("olduğu gibi duruyor"));
    assert_eq!(fs::read(&path).unwrap(), broken);
    assert!(!sidecar(&path, TMP_SUFFIX).exists());
    assert!(!sidecar(&path, OLD_SUFFIX).exists());
}

#[test]
fn yarim_sifreli_kopya_atilir() {
    let dir = TempDir::new("yarim");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    let tmp = sidecar(&path, TMP_SUFFIX);
    fs::write(&tmp, b"yarim kalmis kopya").unwrap();
    fs::write(sidecar(&tmp, "-journal"), b"j").unwrap();

    let s = Store::open(&path, &DbKey::generate().unwrap()).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    assert!(!tmp.exists());
    assert!(!sidecar(&tmp, "-journal").exists());
}

#[test]
fn ad_degisikligi_arasinda_kesilen_donusum_geri_alinir() {
    // Şifresiz dosya kenara alınmış, şifreli kopya henüz asıl adı almamış.
    let dir = TempDir::new("arada");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    let old = sidecar(&path, OLD_SUFFIX);
    fs::rename(&path, &old).unwrap();
    fs::write(sidecar(&path, TMP_SUFFIX), vec![7u8; 4096]).unwrap();

    let key = DbKey::generate().unwrap();
    let s = Store::open(&path, &key).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    drop(s);
    assert!(!old.exists());
    assert!(!sidecar(&path, TMP_SUFFIX).exists());
    assert_no_plaintext(dir.path());
}

#[test]
fn yer_degistirme_sonrasi_kalan_eski_dosya_silinir() {
    let dir = TempDir::new("silme");
    let path = dir.path().join("t.db");
    let key = DbKey::generate().unwrap();
    fill(&Store::open(&path, &key).unwrap());
    // Eski şifresiz dosya ezilip silinmeden kesilmiş.
    let old = sidecar(&path, OLD_SUFFIX);
    fill(&plain_store(&old));

    let s = Store::open(&path, &key).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    drop(s);
    assert!(!old.exists());
    assert_no_plaintext(dir.path());
}

#[test]
fn eski_dosya_yanlis_anahtarla_silinmez() {
    let dir = TempDir::new("silmeyanlis");
    let path = dir.path().join("t.db");
    fill(&Store::open(&path, &DbKey::generate().unwrap()).unwrap());
    let old = sidecar(&path, OLD_SUFFIX);
    fill(&plain_store(&old));
    assert!(Store::open(&path, &DbKey::generate().unwrap()).is_err());
    assert!(
        old.exists(),
        "anahtar doğrulanmadan eski kopyaya dokunulmaz"
    );
}

#[test]
fn iki_duz_kopya_varsa_dokunulmaz() {
    let dir = TempDir::new("ikiduz");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    let old = sidecar(&path, OLD_SUFFIX);
    fill(&plain_store(&old));
    let e = Store::open(&path, &DbKey::generate().unwrap())
        .err()
        .unwrap();
    assert_eq!(e.code, ErrorCode::Storage);
    assert_eq!(encryption::probe(&path).unwrap(), DbFile::Plain);
    assert_eq!(encryption::probe(&old).unwrap(), DbFile::Plain);
}

// ------------------------------------------------------------ anahtar kasası

#[test]
fn ilk_acilista_anahtar_uretilir_ve_kalir() {
    let dir = TempDir::new("kasa");
    let path = dir.path().join("t.db");
    let vault = TestVault::default();
    fill(&Store::open_with_vault(&path, &vault).unwrap());
    let secret = vault.secret.borrow().clone().expect("anahtar kaydedilmeli");
    assert_eq!(secret.len(), 64);
    assert_eq!(encryption::probe(&path).unwrap(), DbFile::Encrypted);

    let s = Store::open_with_vault(&path, &vault).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    assert_eq!(vault.secret.borrow().as_deref(), Some(secret.as_str()));
}

#[test]
fn eski_duz_veritabani_kasayla_sifrelenir() {
    let dir = TempDir::new("kasaduz");
    let path = dir.path().join("t.db");
    fill(&plain_store(&path));
    let vault = TestVault::default();
    let s = Store::open_with_vault(&path, &vault).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    drop(s);
    assert!(vault.secret.borrow().is_some());
    assert_no_plaintext(dir.path());
}

#[test]
fn anahtar_zinciri_yoksa_duz_metne_dusmez() {
    let dir = TempDir::new("zincir");
    let path = dir.path().join("t.db");
    let vault = TestVault::default();
    vault.fail_load.set(true);
    let e = Store::open_with_vault(&path, &vault).err().unwrap();
    assert_eq!(e.code, ErrorCode::Keyring);
    assert!(e.message.contains("şifresiz saklamaz"));
    assert!(!path.exists(), "anahtarsız veritabanı oluşturulmamalı");

    // Önceki sürümden kalan şifresiz veritabanı da olduğu gibi kalır.
    fill(&plain_store(&path));
    let before = fs::read(&path).unwrap();
    assert!(Store::open_with_vault(&path, &vault).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn anahtar_yazilamazsa_ya_da_saklanmazsa_hata() {
    let dir = TempDir::new("yazma");
    let path = dir.path().join("t.db");
    let vault = TestVault::default();
    vault.fail_save.set(true);
    let e = Store::open_with_vault(&path, &vault).err().unwrap();
    assert_eq!(e.code, ErrorCode::Keyring);
    assert!(!path.exists());

    let vault = TestVault::default();
    vault.forget_on_save.set(true);
    let e = Store::open_with_vault(&path, &vault).err().unwrap();
    assert_eq!(e.code, ErrorCode::Keyring);
    assert!(!path.exists());
}

#[test]
fn anahtar_yoksa_sifreli_veri_silinmez_yeni_anahtar_uretilmez() {
    let dir = TempDir::new("anahtaryok");
    let path = dir.path().join("t.db");
    fill(&Store::open_with_vault(&path, &TestVault::default()).unwrap());
    let before = fs::read(&path).unwrap();

    let empty = TestVault::default();
    let e = Store::open_with_vault(&path, &empty).err().unwrap();
    assert_eq!(e, crate::key::key_missing_error(&path));
    assert_eq!(e.code, ErrorCode::Keyring);
    assert!(e.message.contains(&path.display().to_string()));
    assert!(empty.secret.borrow().is_none(), "yeni anahtar üretilmemeli");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn kasadaki_anahtar_uymazsa_hata() {
    let dir = TempDir::new("uymaz");
    let path = dir.path().join("t.db");
    fill(&Store::open_with_vault(&path, &TestVault::default()).unwrap());
    let other = TestVault::default();
    *other.secret.borrow_mut() = Some(DbKey::generate().unwrap().to_secret().to_string());
    let e = Store::open_with_vault(&path, &other).err().unwrap();
    assert_eq!(e, encryption::undecryptable_error(&path));

    *other.secret.borrow_mut() = Some("bozuk".into());
    let e = Store::open_with_vault(&path, &other).err().unwrap();
    assert_eq!(e.code, ErrorCode::Keyring);
    assert!(e.message.contains("bozuk"));
}

#[test]
fn bellek_kipi_yalniz_bos_klasorde_calisir() {
    let dir = TempDir::new("bellek");
    let path = dir.path().join("t.db");
    let vault = TestVault::default();
    vault.ephemeral.set(true);
    fill(&Store::open_with_vault(&path, &vault).unwrap());
    assert_no_plaintext(dir.path());

    // Yeni süreç: anahtar kayboldu, var olan dosyaya dokunulmaz.
    let next = TestVault::default();
    next.ephemeral.set(true);
    let before = fs::read(&path).unwrap();
    let e = Store::open_with_vault(&path, &next).err().unwrap();
    assert_eq!(e.code, ErrorCode::Invalid);
    assert_eq!(fs::read(&path).unwrap(), before);

    // Şifresiz veritabanı bellek kipinde dönüştürülmez (anahtar kaybolurdu).
    let plain = dir.path().join("p.db");
    fill(&plain_store(&plain));
    let e = Store::open_with_vault(&plain, &next).err().unwrap();
    assert_eq!(e.code, ErrorCode::Invalid);
    assert_eq!(encryption::probe(&plain).unwrap(), DbFile::Plain);
}

#[test]
fn bellek_ici_depo_anahtarsiz_calisir() {
    let s = Store::open_in_memory().unwrap();
    fill(&s);
    assert_eq!(found(&s), vec![SUBJECT]);
    let on: i64 = s
        .with(|c| c.query_row("PRAGMA secure_delete", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(on, 1);
}

// ------------------------------------------------------------ silme ve izinler

#[test]
fn hesap_silinince_icerik_dosyada_kalmaz() {
    // Şifresiz dosyada denetlenir: secure_delete + WAL sıfırlama olmadan
    // silinen postanın metni sayfalarda kalırdı.
    let dir = TempDir::new("guvenlisil");
    let path = dir.path().join("t.db");
    let s = plain_store(&path);
    fill(&s);
    assert!(s.remove_account(&"a".to_string()).unwrap());
    assert_eq!(fs::metadata(sidecar(&path, "-wal")).unwrap().len(), 0);
    drop(s);
    for (p, b) in all_bytes(dir.path()) {
        for marker in [SUBJECT.as_bytes(), BODY.as_bytes(), RAW.as_bytes()] {
            assert!(
                !contains(&b, marker),
                "{} içinde silinmiş metin: {}",
                p.display(),
                String::from_utf8_lossy(marker)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn dosya_ve_klasor_izinleri() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;

    let root = TempDir::new("izin");
    let dir = root.path().join("veri");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    encryption::private_dir(&dir).unwrap();
    assert_eq!(mode(&dir), 0o700);
    let nested = root.path().join("yeni/alt");
    encryption::private_dir(&nested).unwrap();
    assert_eq!(mode(&nested), 0o700);

    let path = dir.join("t.db");
    let s = Store::open(&path, &DbKey::generate().unwrap()).unwrap();
    fill(&s);
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&sidecar(&path, "-wal")), 0o600);
    assert_eq!(mode(&sidecar(&path, "-shm")), 0o600);

    // Ortak (yapışkan bitli) klasörlere dokunulmaz.
    let shared = root.path().join("ortak");
    fs::create_dir_all(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).unwrap();
    encryption::private_dir(&shared).unwrap();
    assert_eq!(mode(&shared), 0o1777);
}

// ------------------------------------------------------------ taşıma

#[test]
fn tasima_yan_dosyalarla_yapilir() {
    let root = TempDir::new("tasi");
    let from = plain_with_pending_wal(root.path());
    let to = root.path().join("yerel/alt/t.db");
    assert!(encryption::relocate(&from, &to).unwrap());
    assert!(!from.exists());
    assert!(!sidecar(&from, "-wal").exists());
    assert!(sidecar(&to, "-wal").exists());

    let s = Store::open(&to, &DbKey::generate().unwrap()).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
    // İkinci açılışta taşınacak bir şey yok.
    assert!(!encryption::relocate(&from, &to).unwrap());
}

#[test]
fn yarim_kalan_tasima_surdurulur() {
    let root = TempDir::new("tasiyarim");
    let from = plain_with_pending_wal(root.path());
    let to_dir = root.path().join("yerel");
    fs::create_dir_all(&to_dir).unwrap();
    let to = to_dir.join("t.db");
    // WAL taşınmış, veritabanı taşınmadan kesilmiş.
    fs::rename(sidecar(&from, "-wal"), sidecar(&to, "-wal")).unwrap();

    assert!(encryption::relocate(&from, &to).unwrap());
    let s = Store::open(&to, &DbKey::generate().unwrap()).unwrap();
    assert_eq!(found(&s), vec![SUBJECT]);
}

#[test]
fn tasimada_hedef_varsa_yalniz_ayni_kopya_silinir() {
    let root = TempDir::new("tasiikili");
    let from = root.path().join("a.db");
    let to = root.path().join("b.db");
    fs::write(&from, b"ayni icerik").unwrap();
    fs::write(&to, b"ayni icerik").unwrap();
    assert!(!encryption::relocate(&from, &to).unwrap());
    assert!(
        !from.exists(),
        "taşıma bitmiş, kalan kaynak kopyası silinir"
    );
    assert!(to.exists());

    fs::write(&from, b"farkli icerik").unwrap();
    assert!(!encryption::relocate(&from, &to).unwrap());
    assert!(from.exists(), "farklı veritabanına dokunulmaz");
    assert_eq!(fs::read(&to).unwrap(), b"ayni icerik");

    assert!(!encryption::relocate(&to, &to).unwrap());
    assert!(to.exists());
}
