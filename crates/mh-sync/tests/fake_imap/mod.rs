//! Testler için süreç içi sahte IMAP sunucusu (RFC 3501'in Mailhere'nin
//! kullandığı kısmı; UIDPLUS ve MOVE açılıp kapatılabilir). Şifresiz,
//! yalnız 127.0.0.1 üzerinde dinler.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
pub struct FMsg {
    pub uid: u32,
    pub flags: BTreeSet<String>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FBox {
    pub uidvalidity: u32,
    pub uidnext: u32,
    /// UID sırasıyla.
    pub msgs: Vec<FMsg>,
    /// LIST öznitelikleri (ör. `\Trash`).
    pub attrs: String,
}

impl FBox {
    fn new(attrs: &str) -> Self {
        Self {
            uidvalidity: 7,
            uidnext: 1,
            msgs: Vec::new(),
            attrs: attrs.to_owned(),
        }
    }

    pub fn uids(&self) -> Vec<u32> {
        self.msgs.iter().map(|m| m.uid).collect()
    }

    pub fn flagged(&self, flag: &str) -> Vec<u32> {
        self.msgs
            .iter()
            .filter(|m| m.flags.contains(flag))
            .map(|m| m.uid)
            .collect()
    }
}

#[derive(Default)]
pub struct State {
    pub boxes: BTreeMap<String, FBox>,
    pub uidplus: bool,
    pub has_move: bool,
    /// Gelen komutlar (etiketsiz), sırayla.
    pub log: Vec<String>,
    /// Bayrak değişikliği reddedilsin (salt okunur kutu: `UID STORE` → NO).
    pub read_only: bool,
    /// Aramalar reddedilsin (`UID SEARCH` → NO; geçici sunucu hatası).
    pub search_fails: bool,
}

impl State {
    pub fn add_box(&mut self, name: &str, attrs: &str) {
        self.boxes.insert(name.to_owned(), FBox::new(attrs));
    }

    /// Kutuya posta ekler (başka bir istemciden gelmiş gibi); UID'sini döner.
    pub fn deliver(&mut self, mailbox: &str, raw: Vec<u8>, flags: &[&str]) -> u32 {
        let b = self.boxes.get_mut(mailbox).expect("kutu var");
        let uid = b.uidnext;
        b.uidnext += 1;
        b.msgs.push(FMsg {
            uid,
            flags: flags.iter().map(|f| (*f).to_owned()).collect(),
            raw,
        });
        uid
    }

    pub fn mailbox(&self, name: &str) -> &FBox {
        self.boxes.get(name).expect("kutu var")
    }

    pub fn mailbox_mut(&mut self, name: &str) -> &mut FBox {
        self.boxes.get_mut(name).expect("kutu var")
    }

    /// Verilen sözcükle başlayan komutların sayısı (ör. "UID EXPUNGE").
    pub fn count(&self, prefix: &str) -> usize {
        self.log.iter().filter(|l| l.starts_with(prefix)).count()
    }
}

pub type Shared = Arc<Mutex<State>>;

pub struct Server {
    pub port: u16,
    pub state: Shared,
}

impl Server {
    /// INBOX, Trash ve Junk kutularıyla (UIDPLUS ve MOVE açık) başlar.
    pub async fn start() -> Self {
        let mut st = State {
            uidplus: true,
            has_move: true,
            ..State::default()
        };
        st.add_box("INBOX", "\\HasNoChildren");
        st.add_box("Trash", "\\HasNoChildren \\Trash");
        st.add_box("Junk", "\\HasNoChildren \\Junk");
        let state: Shared = Arc::new(Mutex::new(st));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let shared = state.clone();
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let st = shared.clone();
                tokio::spawn(async move {
                    let _ = serve(sock, st).await;
                });
            }
        });
        Self { port, state }
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_owned()
}

/// "1:3,7,9:*" kümesini (alt, üst) aralıklarına çevirir; `*` en büyük değer.
fn ranges(set: &str, max: u32) -> Vec<(u32, u32)> {
    set.split(',')
        .filter_map(|part| {
            let val = |v: &str| -> Option<u32> {
                if v == "*" {
                    Some(max)
                } else {
                    v.parse().ok()
                }
            };
            let (a, b) = match part.split_once(':') {
                Some((a, b)) => (val(a)?, val(b)?),
                None => {
                    let v = val(part)?;
                    (v, v)
                }
            };
            Some((a.min(b), a.max(b)))
        })
        .collect()
}

fn in_set(set: &[(u32, u32)], v: u32) -> bool {
    set.iter().any(|(a, b)| v >= *a && v <= *b)
}

fn header_of(raw: &[u8]) -> &[u8] {
    match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => &raw[..i + 4],
        None => raw,
    }
}

fn flags_text(m: &FMsg) -> String {
    m.flags.iter().cloned().collect::<Vec<_>>().join(" ")
}

/// FETCH yanıtı (istenen öğelerle).
fn fetch_line(seq: usize, m: &FMsg, items: &str) -> Vec<u8> {
    let up = items.to_ascii_uppercase();
    let mut out = format!("* {seq} FETCH (UID {}", m.uid).into_bytes();
    if up.contains("FLAGS") {
        out.extend(format!(" FLAGS ({})", flags_text(m)).as_bytes());
    }
    if up.contains("INTERNALDATE") {
        out.extend(b" INTERNALDATE \"13-Oct-2026 09:00:00 +0300\"");
    }
    if up.contains("RFC822.SIZE") {
        out.extend(format!(" RFC822.SIZE {}", m.raw.len()).as_bytes());
    }
    if up.contains("BODY.PEEK[HEADER]") {
        let h = header_of(&m.raw);
        out.extend(format!(" BODY[HEADER] {{{}}}\r\n", h.len()).as_bytes());
        out.extend(h);
    } else if up.contains("BODY.PEEK[]") {
        out.extend(format!(" BODY[] {{{}}}\r\n", m.raw.len()).as_bytes());
        out.extend(&m.raw);
    }
    out.extend(b")\r\n");
    out
}

/// Tek bir arama ölçütü.
type Test = Box<dyn Fn(&FMsg) -> bool>;

/// SEARCH ölçütünü uygular (UID kümesi, DELETED, UNDELETED, ALL, HEADER
/// Message-ID; birlikte verilenler VE'lenir).
fn search(b: &FBox, criteria: &str) -> Vec<u32> {
    let max = b.msgs.last().map_or(0, |m| m.uid);
    let mut tests: Vec<Test> = Vec::new();
    let words: Vec<&str> = criteria.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        match words[i].to_ascii_uppercase().as_str() {
            "ALL" => {}
            "DELETED" => tests.push(Box::new(|m| m.flags.contains("\\Deleted"))),
            "UNDELETED" => tests.push(Box::new(|m| !m.flags.contains("\\Deleted"))),
            "UID" => {
                i += 1;
                let set = ranges(words.get(i).copied().unwrap_or(""), max);
                tests.push(Box::new(move |m| in_set(&set, m.uid)));
            }
            "HEADER" => {
                // HEADER Message-ID "<kimlik>"
                let value = unquote(&words[i + 2..].join(" "));
                let needle = value.to_ascii_lowercase();
                tests.push(Box::new(move |m| {
                    String::from_utf8_lossy(header_of(&m.raw))
                        .to_ascii_lowercase()
                        .lines()
                        .any(|l| l.starts_with("message-id:") && l.contains(&needle))
                }));
                i = words.len();
            }
            _ => {}
        }
        i += 1;
    }
    b.msgs
        .iter()
        .filter(|m| tests.iter().all(|t| t(m)))
        .map(|m| m.uid)
        .collect()
}

async fn serve(sock: tokio::net::TcpStream, state: Shared) -> std::io::Result<()> {
    let (r, mut w) = sock.into_split();
    let mut lines = BufReader::new(r).lines();
    w.write_all(b"* OK sahte IMAP hazir\r\n").await?;
    let mut selected: Option<String> = None;
    while let Some(line) = lines.next_line().await? {
        let (tag, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let upper = rest.to_ascii_uppercase();
        let mut out: Vec<u8> = Vec::new();
        let mut done = format!("{tag} OK tamam\r\n");
        let mut bye = false;
        {
            let mut st = state.lock().unwrap();
            if !upper.starts_with("LOGIN") {
                st.log.push(rest.to_owned());
            }
            let st = &mut *st;
            if upper.starts_with("CAPABILITY") {
                let mut caps = String::from("* CAPABILITY IMAP4rev1");
                if st.uidplus {
                    caps += " UIDPLUS";
                }
                if st.has_move {
                    caps += " MOVE";
                }
                out.extend(format!("{caps}\r\n").as_bytes());
            } else if upper.starts_with("LOGIN") || upper.starts_with("NOOP") {
            } else if upper.starts_with("LOGOUT") {
                out.extend(b"* BYE gorusuruz\r\n");
                bye = true;
            } else if upper.starts_with("LIST") {
                for (name, b) in &st.boxes {
                    out.extend(format!("* LIST ({}) \"/\" \"{name}\"\r\n", b.attrs).as_bytes());
                }
            } else if upper.starts_with("CREATE ") {
                let name = unquote(&rest[7..]);
                st.boxes
                    .entry(name)
                    .or_insert_with(|| FBox::new("\\HasNoChildren"));
            } else if upper.starts_with("SELECT ") || upper.starts_with("EXAMINE ") {
                let name = unquote(rest.split_once(' ').map_or("", |x| x.1));
                match st.boxes.get(&name) {
                    Some(b) => {
                        out.extend(
                            format!(
                                "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n\
                                 * {} EXISTS\r\n* 0 RECENT\r\n\
                                 * OK [UIDVALIDITY {}] tamam\r\n* OK [UIDNEXT {}] tamam\r\n",
                                b.msgs.len(),
                                b.uidvalidity,
                                b.uidnext
                            )
                            .as_bytes(),
                        );
                        selected = Some(name);
                    }
                    None => {
                        selected = None;
                        done = format!("{tag} NO kutu yok\r\n");
                    }
                }
            } else if let Some(name) = selected.clone() {
                let max_uid = st.boxes[&name].msgs.last().map_or(0, |m| m.uid);
                let count = st.boxes[&name].msgs.len() as u32;
                if upper.starts_with("UID SEARCH ") && st.search_fails {
                    done = format!("{tag} NO arama yapilamadi\r\n");
                } else if upper.starts_with("UID SEARCH ") {
                    let found = search(&st.boxes[&name], &rest["UID SEARCH ".len()..]);
                    let mut line = String::from("* SEARCH");
                    for uid in found {
                        line += &format!(" {uid}");
                    }
                    out.extend(format!("{line}\r\n").as_bytes());
                } else if upper.starts_with("UID FETCH ") || upper.starts_with("FETCH ") {
                    let by_uid = upper.starts_with("UID ");
                    let args = &rest[if by_uid {
                        "UID FETCH ".len()
                    } else {
                        "FETCH ".len()
                    }..];
                    let (set, items) = args.split_once(' ').unwrap_or((args, ""));
                    let set = ranges(set, if by_uid { max_uid } else { count });
                    for (i, m) in st.boxes[&name].msgs.iter().enumerate() {
                        let key = if by_uid { m.uid } else { i as u32 + 1 };
                        if in_set(&set, key) {
                            out.extend(fetch_line(i + 1, m, items));
                        }
                    }
                } else if upper.starts_with("UID STORE ") && st.read_only {
                    done = format!("{tag} NO kutu salt okunur\r\n");
                } else if upper.starts_with("UID STORE ") {
                    let args = &rest["UID STORE ".len()..];
                    let mut parts = args.splitn(3, ' ');
                    let set = ranges(parts.next().unwrap_or(""), max_uid);
                    let op = parts.next().unwrap_or("").to_owned();
                    let flags: Vec<String> = parts
                        .next()
                        .unwrap_or("")
                        .trim_matches(|c| c == '(' || c == ')')
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    let b = st.boxes.get_mut(&name).expect("seçili kutu");
                    for m in b.msgs.iter_mut().filter(|m| in_set(&set, m.uid)) {
                        for f in &flags {
                            if op.starts_with('+') {
                                m.flags.insert(f.clone());
                            } else if op.starts_with('-') {
                                m.flags.remove(f);
                            }
                        }
                    }
                } else if upper.starts_with("UID EXPUNGE ") || upper.starts_with("EXPUNGE") {
                    let only = upper
                        .strip_prefix("UID EXPUNGE ")
                        .map(|set| ranges(set.trim(), max_uid));
                    if only.is_some() && !st.uidplus {
                        done = format!("{tag} BAD UIDPLUS yok\r\n");
                    } else {
                        let b = st.boxes.get_mut(&name).expect("seçili kutu");
                        let mut seq = 1;
                        b.msgs.retain(|m| {
                            let gone = m.flags.contains("\\Deleted")
                                && only.as_ref().is_none_or(|s| in_set(s, m.uid));
                            if gone {
                                out.extend(format!("* {seq} EXPUNGE\r\n").as_bytes());
                            } else {
                                seq += 1;
                            }
                            !gone
                        });
                    }
                } else if upper.starts_with("UID MOVE ") || upper.starts_with("UID COPY ") {
                    let moving = upper.starts_with("UID MOVE ");
                    let args = &rest["UID MOVE ".len()..];
                    let (set, dest) = args.split_once(' ').unwrap_or((args, ""));
                    let dest = unquote(dest);
                    let set = ranges(set, max_uid);
                    if moving && !st.has_move {
                        done = format!("{tag} BAD MOVE yok\r\n");
                    } else if !st.boxes.contains_key(&dest) {
                        done = format!("{tag} NO [TRYCREATE] hedef yok\r\n");
                    } else {
                        let b = st.boxes.get_mut(&name).expect("seçili kutu");
                        let picked: Vec<FMsg> = b
                            .msgs
                            .iter()
                            .filter(|m| in_set(&set, m.uid))
                            .cloned()
                            .collect();
                        if moving {
                            b.msgs.retain(|m| !in_set(&set, m.uid));
                        }
                        let d = st.boxes.get_mut(&dest).expect("hedef kutu");
                        for mut m in picked {
                            m.uid = d.uidnext;
                            d.uidnext += 1;
                            m.flags.remove("\\Deleted");
                            d.msgs.push(m);
                        }
                    }
                } else {
                    done = format!("{tag} BAD bilinmeyen komut\r\n");
                }
            } else {
                done = format!("{tag} BAD kutu secili degil\r\n");
            }
        }
        out.extend(done.as_bytes());
        w.write_all(&out).await?;
        if bye {
            break;
        }
    }
    Ok(())
}
