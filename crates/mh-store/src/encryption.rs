//! Diskteki veritabanının şifrelenmesi (SQLCipher 4: AES-256, sayfa başına
//! HMAC-SHA512).
//!
//! - Anahtar bağlantı açılır açılmaz, başka hiçbir deyimden önce verilir
//!   (`sqlite3_key_v2`; anahtar SQL metnine girmez).
//! - Önceki sürümlerden kalan şifresiz veritabanı ilk açılışta şifreli bir
//!   kopyaya aktarılır, doğrulanır ve atomik olarak yerine konur. Her adımda
//!   kesilse bile bir sonraki açılış ya sağlam şifresiz dosyayı ya da tam
//!   şifreli dosyayı görür (`recover`).
//! - Unix'te veritabanı dosyaları yalnız sahibine açıktır (0600), veri klasörü
//!   0700. Windows'ta kullanıcı profilinin varsayılan erişim listesi yeterlidir.

use std::ffi::c_int;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use mh_core::{AppError, AppResult, ErrorCode};
use rusqlite::types::Value;
use rusqlite::{ffi, Connection};

use crate::key::DbKey;

/// Şifresiz SQLite dosyasının ilk 16 baytı. Şifreli dosya rastgele tuzla başlar.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Dönüşüm sırasında yazılan şifreli kopya.
pub(crate) const TMP_SUFFIX: &str = ".sifreleniyor";
/// Yerinden alınmış, ezilip silinmeyi bekleyen eski şifresiz dosya.
pub(crate) const OLD_SUFFIX: &str = ".sifresiz-eski";
/// SQLite'ın yan dosyaları.
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// Diskteki veritabanı dosyasının durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbFile {
    /// Yok ya da boş: yeni veritabanı.
    Missing,
    /// Önceki sürümlerden kalma şifresiz SQLite dosyası.
    Plain,
    /// SQLite başlığı yok: şifreli (ya da tanınmayan) dosya.
    Encrypted,
}

/// Dosyanın başlığına bakarak durumunu söyler; dosyayı değiştirmez.
pub fn probe(path: &Path) -> io::Result<DbFile> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(DbFile::Missing),
        Err(e) => return Err(e),
    };
    let mut head = [0u8; 16];
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(match n {
        0 => DbFile::Missing,
        16 if &head == SQLITE_MAGIC => DbFile::Plain,
        _ => DbFile::Encrypted,
    })
}

pub(crate) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

pub(crate) fn io_error(path: &Path, e: &io::Error) -> AppError {
    tracing::error!(path = %path.display(), error = %e, "veritabanı dosyası işlenemedi");
    AppError::new(
        ErrorCode::Storage,
        format!(
            "Yerel veritabanı dosyasına ({}) erişilemedi. Klasörün yazılabilir olduğunu ve diskte yer bulunduğunu denetleyip Mailhere'yi yeniden başlat.",
            path.display()
        ),
    )
}

/// Bağlantıyı açar ve anahtarı verir; anahtarın dosyaya uyduğunu bir okuma
/// ile doğrular (uymazsa `SQLITE_NOTADB`).
pub(crate) fn open_keyed(path: &Path, key: &DbKey) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    let literal = key.raw_key_literal();
    let len = c_int::try_from(literal.len()).map_err(|_| misuse())?;
    // SAFETY: `conn` açık ve geçerli bir bağlantı, tanıtıcısı bu çağrı
    // boyunca başka yerde kullanılmıyor. Anahtar arabelleği çağrı süresince
    // yaşar; SQLCipher kendi kopyasını alır (ve bırakırken sıfırlar).
    let rc = unsafe {
        ffi::sqlite3_key_v2(
            conn.handle(),
            c"main".as_ptr(),
            literal.as_ptr().cast(),
            len,
        )
    };
    if rc != ffi::SQLITE_OK {
        return Err(rusqlite::Error::SqliteFailure(ffi::Error::new(rc), None));
    }
    // Biçim sabit: ileride SQLCipher varsayılanları değişse de dosya açılır.
    conn.pragma_update(None, "cipher_compatibility", 4)?;
    conn.query_row("SELECT count(*) FROM sqlite_schema", [], |r| {
        r.get::<_, i64>(0)
    })?;
    Ok(conn)
}

fn misuse() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(ffi::Error::new(ffi::SQLITE_MISUSE), None)
}

/// Anahtar uymadı ya da dosya bozuk mu?
pub(crate) fn is_undecryptable(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(
            ffi::Error {
                code: rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt,
                ..
            },
            _
        )
    )
}

pub(crate) fn undecryptable_error(path: &Path) -> AppError {
    tracing::error!(path = %path.display(), "veritabanının şifresi çözülemedi");
    AppError::new(
        ErrorCode::Storage,
        format!(
            "Yerel veritabanının şifresi çözülemedi: anahtar zincirindeki anahtar bu dosyaya uymuyor ya da dosya bozulmuş. Dosya silinmedi. Anahtar zincirini yedekten geri yükleyebilirsin; baştan başlamak istersen {} dosyasını başka bir klasöre taşıyıp Mailhere'yi yeniden başlat (hesaplarını yeniden eklemen gerekir).",
            path.display()
        ),
    )
}

/// Veritabanını ve tüm yan dosyalarını (WAL, SHM, günlük, yarım kalmış
/// dönüşüm kopyaları) siler. Yalnız kullanıcı yerel veriyi bilerek
/// sıfırlayınca (parola unutuldu) çağrılır; olmayan dosya hata değildir.
/// Şifreli dosyanın anahtarı da silindiğinden içerik anahtarsız okunamaz;
/// diskteki blokların üzerine yazılmaz (SSD'de güvenilir değildir).
pub fn remove_database(path: &Path) -> io::Result<()> {
    let mut targets = vec![path.to_path_buf()];
    targets.extend(SIDECARS.iter().map(|s| sidecar(path, s)));
    targets.push(sidecar(path, TMP_SUFFIX));
    targets.push(sidecar(path, OLD_SUFFIX));
    for t in targets {
        match fs::remove_file(&t) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Yarıda kalmış bir dönüşümün izlerini toplar. Anahtar gerektirmez ve hiçbir
/// durumda kullanıcı verisi içeren tek kopyayı silmez.
///
/// Dönüşüm sırası: şifreli kopya (`.sifreleniyor`) yazılır ve doğrulanır →
/// şifresiz dosya `.sifresiz-eski` adını alır → kopya asıl adı alır → eski
/// dosya ezilip silinir.
pub(crate) fn recover(path: &Path) -> AppResult<()> {
    let old = sidecar(path, OLD_SUFFIX);
    let tmp = sidecar(path, TMP_SUFFIX);
    if old.exists() {
        match probe(path).map_err(|e| io_error(path, &e))? {
            // İki ad değişikliği arasında kesilmiş: şifresiz dosya geri konur,
            // dönüşüm bu açılışta baştan yapılır.
            DbFile::Missing => {
                tracing::warn!("yarıda kalan şifreleme geri alınıyor");
                if path.exists() {
                    // Boş dosya: veri içermez.
                    fs::remove_file(path).map_err(|e| io_error(path, &e))?;
                }
                rename(&old, path).map_err(|e| io_error(path, &e))?;
            }
            // Yer değiştirme bitmiş; eski dosya, anahtarla açılış doğrulandıktan
            // sonra `finish` içinde silinir.
            DbFile::Encrypted => {}
            DbFile::Plain => {
                tracing::error!("iki şifresiz veritabanı kopyası var");
                return Err(AppError::new(
                    ErrorCode::Storage,
                    format!(
                        "Veri klasöründe iki şifresiz veritabanı var ({} ve {}); hangisinin güncel olduğu bilinmediği için ikisine de dokunulmadı. Birini başka bir klasöre taşıyıp Mailhere'yi yeniden başlat.",
                        path.display(),
                        old.display()
                    ),
                ));
            }
        }
    }
    // Yarım (ya da doğrulanmış ama yerine konmamış) şifreli kopya: asıl dosya
    // sağlam olduğundan atılır; dönüşüm gerekirse yeniden yapılır.
    if tmp.exists() {
        tracing::warn!("yarım kalan şifreli kopya siliniyor");
        remove_db_files(&tmp).map_err(|e| io_error(&tmp, &e))?;
    }
    Ok(())
}

/// Açılıştan önce: yarım dönüşümü toparlar, şifresiz dosyayı şifreler, yeni
/// dosyayı yalnız sahibine açık oluşturur.
pub(crate) fn prepare(path: &Path, key: &DbKey) -> AppResult<()> {
    recover(path)?;
    match probe(path).map_err(|e| io_error(path, &e))? {
        DbFile::Plain => {
            let started = Instant::now();
            tracing::info!(path = %path.display(), "şifresiz veritabanı şifreleniyor");
            encrypt_in_place(path, key).map_err(|e| {
                tracing::error!(error = %e, "şifresiz veritabanı şifrelenemedi");
                // Atılan kopya bir sonraki açılışta da temizlenir.
                let _ = remove_db_files(&sidecar(path, TMP_SUFFIX));
                AppError::new(
                    ErrorCode::Storage,
                    "Önceki sürümden kalan şifresiz veritabanı şifreli biçime dönüştürülemedi; veriler olduğu gibi duruyor. Diskte veritabanı kadar boş alan olduğundan emin olup Mailhere'yi yeniden başlat.",
                )
            })?;
            tracing::info!(
                ms = started.elapsed().as_millis() as u64,
                "veritabanı şifrelendi"
            );
        }
        DbFile::Missing if !path.exists() => {
            create_private_file(path).map_err(|e| io_error(path, &e))?;
        }
        DbFile::Missing | DbFile::Encrypted => {}
    }
    harden_files(path);
    Ok(())
}

/// Anahtarla açılış doğrulandıktan sonra: dönüşümden kalan eski şifresiz
/// dosya (silme yarıda kaldıysa) ezilip silinir.
pub(crate) fn finish(path: &Path) {
    let old = sidecar(path, OLD_SUFFIX);
    if old.exists() {
        tracing::info!("eski şifresiz veritabanı siliniyor");
        if let Err(e) = wipe(&old) {
            tracing::warn!(error = %e, "eski şifresiz veritabanı silinemedi; sonraki açılışta yeniden denenecek");
        }
    }
    harden_files(path);
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Şifresiz veritabanını şifreli kopyaya aktarır, doğrular ve yerine koyar.
fn encrypt_in_place(path: &Path, key: &DbKey) -> Result<(), BoxError> {
    let tmp = sidecar(path, TMP_SUFFIX);
    let old = sidecar(path, OLD_SUFFIX);
    remove_db_files(&tmp)?;

    // 1. WAL'daki her şey ana dosyaya geçsin; -wal/-shm kalmasın (yoksa yer
    //    değiştirince yeni dosyaya uygulanmaya çalışılır).
    let (version, objects) = {
        let plain = Connection::open(path)?;
        plain.busy_timeout(std::time::Duration::from_secs(5))?;
        let mode: String = plain.query_row("PRAGMA journal_mode = DELETE", [], |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(format!("günlük kipi değiştirilemedi: {mode}").into());
        }
        schema_fingerprint(&plain, "main")?
    };
    if fs::metadata(sidecar(path, "-wal")).is_ok_and(|m| m.len() > 0) {
        return Err("WAL dosyası boşaltılamadı".into());
    }
    for s in ["-wal", "-shm"] {
        remove_if_exists(&sidecar(path, s))?;
    }

    // 2. Şifreli kopya. Anahtar yalnız `open_keyed` ile verilir; şifresiz
    //    kaynak boş anahtarla (KEY '') bağlanır.
    create_private_file(&tmp)?;
    {
        let enc = open_keyed(&tmp, key)?;
        enc.execute("ATTACH DATABASE ?1 AS plain KEY ''", [path_value(path)?])?;
        // Bağlanan dosya gerçekten açtığımız dosya mı? (Yanlış yol boş bir
        // veritabanı yaratır ve boş kopya şifresiz verinin yerine geçerdi.)
        if schema_fingerprint(&enc, "plain")? != (version, objects) {
            return Err("bağlanan kaynak veritabanı beklenenle aynı değil".into());
        }
        enc.query_row("SELECT sqlcipher_export('main', 'plain')", [], |_| Ok(()))?;
        enc.pragma_update(None, "user_version", version)?;
        verify_row_counts(&enc)?;
        enc.execute("DETACH DATABASE plain", [])?;
    }

    // 3. Diskten yeniden açıp doğrula, kalıcı yaz.
    {
        let enc = open_keyed(&tmp, key)?;
        let check: String = enc.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(format!("şifreli kopya denetimi: {check}").into());
        }
        if schema_fingerprint(&enc, "main")? != (version, objects) {
            return Err("şifreli kopyanın şeması tutmuyor".into());
        }
    }
    for s in SIDECARS {
        remove_if_exists(&sidecar(&tmp, s))?;
    }
    sync_file(&tmp)?;

    // 4. Yer değiştir: önce eski dosya kenara, sonra kopya asıl ada. Arada
    //    kesilirse `recover` eski dosyayı geri koyar.
    rename(path, &old)?;
    if let Err(e) = rename(&tmp, path) {
        let _ = rename(&old, path);
        return Err(e.into());
    }
    sync_dir(path);

    // 5. Eski şifresiz dosyayı ez ve sil (en iyi çaba; olmazsa `finish` ve
    //    sonraki açılışlar yeniden dener).
    if let Err(e) = wipe(&old) {
        tracing::warn!(error = %e, "eski şifresiz veritabanı silinemedi");
    }
    Ok(())
}

/// Şema sürümü ve şema nesnesi sayısı: kaynakla kopyanın aynı olduğunu
/// doğrulamak için.
fn schema_fingerprint(conn: &Connection, db: &str) -> rusqlite::Result<(i64, i64)> {
    let version = conn.query_row(&format!("PRAGMA {db}.user_version"), [], |r| r.get(0))?;
    let objects = conn.query_row(
        &format!("SELECT count(*) FROM {db}.sqlite_schema"),
        [],
        |r| r.get(0),
    )?;
    Ok((version, objects))
}

/// Her tablonun satır sayısı kaynakta ve kopyada aynı mı?
fn verify_row_counts(conn: &Connection) -> Result<(), BoxError> {
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM plain.sqlite_schema WHERE type = 'table' AND rootpage > 0")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for t in tables {
        let quoted = format!("\"{}\"", t.replace('"', "\"\""));
        let (a, b): (i64, i64) = conn.query_row(
            &format!(
                "SELECT (SELECT count(*) FROM plain.{quoted}), (SELECT count(*) FROM main.{quoted})"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if a != b {
            return Err(format!("{t} tablosu eksik aktarıldı ({b}/{a})").into());
        }
    }
    Ok(())
}

/// `ATTACH` için dosya yolu. Unix'te UTF-8 olmayan yol baytları olduğu gibi
/// geçer (SQLite metni bayt olarak okur).
fn path_value(path: &Path) -> Result<Value, BoxError> {
    if let Some(s) = path.to_str() {
        return Ok(Value::Text(s.to_owned()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(Value::Blob(path.as_os_str().as_bytes().to_vec()))
    }
    #[cfg(not(unix))]
    {
        Err("veritabanı yolu Unicode değil".into())
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Veritabanı dosyasını ve SQLite yan dosyalarını siler.
fn remove_db_files(path: &Path) -> io::Result<()> {
    remove_if_exists(path)?;
    for s in SIDECARS {
        remove_if_exists(&sidecar(path, s))?;
    }
    Ok(())
}

/// Dosyayı sıfırlarla ezip siler. En iyi çabadır: SSD'lerde ve yazarken
/// kopyalayan dosya sistemlerinde (btrfs, ZFS, APFS) eski bloklar fiziksel
/// olarak bir süre kalabilir; tam koruma için disk şifrelemesi gerekir.
fn wipe(path: &Path) -> io::Result<()> {
    let overwrite = || -> io::Result<()> {
        let mut f = OpenOptions::new().write(true).open(path)?;
        let mut left = f.metadata()?.len();
        let zeros = vec![0u8; 64 * 1024];
        while left > 0 {
            let n = left.min(zeros.len() as u64) as usize;
            f.write_all(&zeros[..n])?;
            left -= n as u64;
        }
        f.sync_all()
    };
    if let Err(e) = overwrite() {
        tracing::warn!(error = %e, "eski dosya ezilemedi; yalnız siliniyor");
    }
    remove_db_files(path)?;
    sync_dir(path);
    Ok(())
}

/// Ad değiştirir. Windows'ta virüs tarayıcısı gibi bir süreç yeni yazılmış
/// dosyayı kısa süre açık tutabildiğinden birkaç kez yeniden denenir.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0u64;
    loop {
        match fs::rename(from, to) {
            Err(e)
                if cfg!(windows)
                    && attempt < 8
                    && !matches!(
                        e.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::CrossesDevices
                    ) =>
            {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(50 * attempt));
            }
            other => return other,
        }
    }
}

/// Dosyayı diske yazar. Windows'ta salt okunur tanıtıcıyla yapılamadığından
/// yazma kipinde açılır.
fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()
}

/// Ad değişikliklerinin kalıcı olması için klasörü diske yazar (Unix).
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        if let Err(e) = File::open(dir).and_then(|d| d.sync_all()) {
            tracing::warn!(error = %e, "veri klasörü diske yazılamadı");
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Yalnız sahibine açık (Unix: 0600) boş dosya oluşturur. SQLite `-wal`,
/// `-shm` ve `-journal` dosyalarını veritabanı dosyasının izinleriyle açar.
fn create_private_file(path: &Path) -> io::Result<()> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path).map(drop)
}

/// Var olan veritabanı dosyalarını yalnız sahibine açar (önceki sürümler
/// 0644 ile oluşturuyordu).
fn harden_files(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let files =
            std::iter::once(path.to_path_buf()).chain(SIDECARS.iter().map(|s| sidecar(path, s)));
        for f in files {
            let Ok(meta) = fs::metadata(&f) else { continue };
            if meta.permissions().mode() & 0o077 != 0 {
                if let Err(e) = fs::set_permissions(&f, fs::Permissions::from_mode(0o600)) {
                    tracing::warn!(error = %e, "veritabanı dosyasının izinleri daraltılamadı");
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Veritabanını yan dosyalarıyla eski konumundan yenisine taşır (ör. Windows'ta
/// dolaşan profil klasöründen, Roaming, yerel klasöre). Önce yan dosyalar, en
/// son veritabanı taşınır; kaynak ancak hedef yerine konduktan sonra silinir.
/// Kesilirse sonraki açılışta kaldığı yerden sürer. İki yol aynı yere
/// çıkıyorsa (Linux, macOS) hiçbir şey yapmaz. Taşıdıysa `true`.
pub fn relocate(from: &Path, to: &Path) -> AppResult<bool> {
    if from == to || !from.exists() {
        return Ok(false);
    }
    let fail = |e: io::Error| {
        tracing::error!(error = %e, from = %from.display(), to = %to.display(), "veritabanı taşınamadı");
        AppError::new(
            ErrorCode::Storage,
            format!(
                "Yerel veritabanı eski klasöründen ({}) yeni klasörüne ({}) taşınamadı; veriler eski yerinde duruyor ve taşıma bir sonraki açılışta sürdürülür. Diskte yer olduğunu ve klasörlerin yazılabilir olduğunu denetleyip Mailhere'yi yeniden başlat.",
                from.display(),
                to.display()
            ),
        )
    };
    if to.exists() {
        // Aynı dosyaya çıkan iki yol (bağlantı): taşınacak bir şey yok.
        if fs::canonicalize(from).map_err(fail)? == fs::canonicalize(to).map_err(fail)? {
            return Ok(false);
        }
        // Taşıma bitmiş, yalnız kaynak silinememiş: içerik aynıysa kaynak gider.
        if same_contents(from, to).map_err(fail)? {
            remove_db_files(from).map_err(fail)?;
        } else {
            tracing::warn!(from = %from.display(), "eski konumda başka bir veritabanı duruyor; dokunulmadı");
        }
        return Ok(false);
    }
    if let Some(dir) = to.parent() {
        private_dir(dir).map_err(fail)?;
    }
    for s in SIDECARS {
        let src = sidecar(from, s);
        if src.exists() {
            move_file(&src, &sidecar(to, s)).map_err(fail)?;
        }
    }
    move_file(from, to).map_err(fail)?;
    tracing::info!(from = %from.display(), to = %to.display(), "veritabanı yerel klasöre taşındı");
    Ok(true)
}

/// Aynı birimde ad değiştirir; olmazsa (farklı birim, ör. ağ paylaşımına
/// yönlendirilmiş profil) kopyalar, diske yazar, yerine koyar ve kaynağı siler.
fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    if rename(src, dst).is_ok() {
        sync_dir(dst);
        return Ok(());
    }
    let part = sidecar(dst, ".tasiniyor");
    fs::copy(src, &part)?;
    sync_file(&part)?;
    rename(&part, dst)?;
    sync_dir(dst);
    fs::remove_file(src)
}

fn same_contents(a: &Path, b: &Path) -> io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

/// Veri klasörünü oluşturur ve (Unix'te) yalnız sahibine açar: 0700.
/// Yapışkan bitli ortak klasörlere (ör. `/tmp`) dokunulmaz. Windows'ta
/// kullanıcı profilindeki klasörün varsayılan erişim listesi yalnız o
/// kullanıcıya (ve yöneticilere) açıktır.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let mode = fs::metadata(dir)?.permissions().mode();
        if mode & 0o1000 == 0 && mode & 0o077 != 0 {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}
