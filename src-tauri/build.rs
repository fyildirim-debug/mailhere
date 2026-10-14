/// Ortak komutlar (mh-app) ve yalnız masaüstü komutları; satır başına bir ad,
/// `#` ile başlayan satırlar yorum.
const SHARED: &str = include_str!("../crates/mh-app/commands.txt");
const DESKTOP: &str = include_str!("commands.txt");

fn names(list: &str) -> impl Iterator<Item = &str> {
    list.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

fn main() {
    println!("cargo:rerun-if-changed=../crates/mh-app/commands.txt");
    println!("cargo:rerun-if-changed=commands.txt");
    // Uygulama komutları için izin tanımları: her pencere yalnız kendi
    // yetenek dosyasında (`capabilities/*.json`) izin verilen komutları
    // çağırabilir. Yeni ortak komut `crates/mh-app/commands.txt`e (ve
    // `shared_commands!` listesine), masaüstü komutu `commands.txt`e ve
    // ilgili pencerenin yetenek dosyasına eklenir.
    // `commands` dilim ömrünü `'static` ister; liste derleme boyunca yaşar.
    let commands: &'static [&'static str] = Box::leak(
        names(SHARED)
            .chain(names(DESKTOP))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("Tauri derleme adımı başarısız");
}
