//! Takvim daveti: iCalendar (RFC 5545) ayrıştırma ve iMIP (RFC 6047) yanıtı.
//!
//! İçerik göndericinin elindedir: parça boyutu, satır sayısı, satır boyu ve
//! metin alanları sınırlıdır; bozuk satırlar sessizce atlanır, hiçbir girdi
//! panik üretmez. Zaman: `Z` ile biten değer UTC; `TZID` için postadaki
//! VTIMEZONE (yıllık STANDARD/DAYLIGHT kuralları) ya da adındaki açık
//! kayma ("UTC+03:00"); bilinmeyen TZID ve dilimsiz ("floating") değer bu
//! bilgisayarın yerel saati. Tüm gün etkinlik günün UTC 00:00'ı olarak döner.

use chrono::{
    Datelike, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta,
    TimeZone, Utc, Weekday,
};
use mail_parser::{MessageParser, MimeHeaders};
use mh_core::{Address, Invite, InviteMethod, InviteResponse, Millis};

use crate::text::{clean_display_name, clean_line};

/// İncelenen en büyük takvim parçası (bayt); büyüğü yok sayılır.
pub const MAX_ICS: usize = 512 * 1024;
/// Katlama açıldıktan sonra okunan en çok satır.
const MAX_LINES: usize = 20_000;
/// Bir satırın (katlama açılmış) en büyük boyu (bayt); fazlası atılır.
const MAX_LINE: usize = 16 * 1024;
/// Metin alanlarının (ad, yer, UID) üst sınırı (karakter).
const MAX_TEXT: usize = 500;
/// Sayılan en çok katılımcı.
const MAX_ATTENDEES: usize = 2_000;
/// Bir adresin üst sınırı (karakter).
const MAX_EMAIL: usize = 254;
/// Katlanmış satırın en büyük boyu (bayt, RFC 5545 §3.1).
const FOLD_AT: usize = 75;

/// Çözümlenmiş bir içerik satırı (`AD;PARAM=DEĞER:değer`).
struct Line {
    /// Büyük harfli özellik adı.
    name: String,
    /// Büyük harfli parametre adı ve tırnakları açılmış değeri.
    params: Vec<(String, String)>,
    value: String,
    /// Katlaması açılmış özgün satır (yanıtta aynen kopyalanan özellikler için).
    raw: String,
}

impl Line {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Bir tarih ya da tarih-saat değeri, dilimi henüz çözülmemiş.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    date: NaiveDate,
    /// `None`: tüm gün (VALUE=DATE).
    time: Option<NaiveTime>,
    utc: bool,
    tzid: Option<String>,
}

/// Dilim kuralındaki bir geçiş (STANDARD ya da DAYLIGHT).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Observance {
    /// İlk geçiş (önceki kaymanın yerel saatiyle).
    start: NaiveDateTime,
    /// Geçişten sonraki kayma (saniye).
    to: i32,
    /// Geçişten önceki kayma (saniye).
    from: i32,
    rule: Option<YearlyRule>,
}

/// `RRULE:FREQ=YEARLY;BYMONTH=…;BYDAY=…` (yalnız yıllık kurallar).
#[derive(Debug, Clone, PartialEq, Eq)]
struct YearlyRule {
    month: u32,
    /// (sıra, gün): `-1SU` son pazar, `2SU` ikinci pazar.
    byday: Option<(i8, Weekday)>,
    monthday: Option<u32>,
    until: Option<NaiveDateTime>,
}

/// Postadaki VTIMEZONE bileşeni.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Zone {
    id: String,
    observances: Vec<Observance>,
}

/// Davet edilen bir kişi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub address: Address,
    /// PARTSTAT parametresi (büyük harfli), yoksa `None`.
    pub partstat: Option<String>,
}

/// Postadaki ilk VEVENT.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: Option<String>,
    pub organizer: Option<Address>,
    pub attendees: Vec<Attendee>,
    pub sequence: u32,
    start: Option<Stamp>,
    end: Option<Stamp>,
    /// DURATION (saniye); DTEND yoksa kullanılır.
    duration: Option<i64>,
    /// Yanıtta aynen kopyalanan satırlar.
    raw_start: Option<String>,
    raw_end: Option<String>,
    raw_recurrence_id: Option<String>,
}

/// Ayrıştırılmış takvim: METHOD, ilk etkinlik ve dilim tanımları.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Calendar {
    /// Büyük harfli METHOD (REQUEST, CANCEL, REPLY, PUBLISH…).
    pub method: Option<String>,
    pub event: Option<Event>,
    zones: Vec<Zone>,
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((pos, _)) => s[..pos].to_owned(),
        None => s.to_owned(),
    }
}

/// `\n`, `\,`, `\;`, `\\` kaçışlarını açar.
fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(x) => out.push(x),
            None => {}
        }
    }
    out
}

/// Gösterilecek tek satırlık metin: kaçışlar açılır, denetim ve yön
/// karakterleri atılır, uzunluk sınırlanır.
fn text(v: &str) -> String {
    truncate_chars(clean_line(&unescape(v)).trim(), MAX_TEXT)
}

/// Katlanmış satırları birleştirir (devam satırı boşluk ya da sekmeyle başlar).
fn unfold(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in input.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                if last.len() + rest.len() <= MAX_LINE {
                    last.push_str(rest);
                }
                continue;
            }
        }
        if out.len() >= MAX_LINES {
            break;
        }
        if line.len() > MAX_LINE {
            // Uzunluk sınırı karakter sınırında kesilir.
            let cut = line
                .char_indices()
                .take_while(|(i, _)| *i <= MAX_LINE)
                .last()
                .map_or(0, |(i, _)| i);
            out.push(line[..cut].to_owned());
        } else {
            out.push(line.to_owned());
        }
    }
    out
}

/// `AD;P=V;Q="a:b":değer` satırını çözer. Tırnak içindeki `;` ve `:` ayraç
/// sayılmaz; değer ilk tırnaksız `:` sonrasının tamamıdır.
fn parse_line(raw: &str) -> Option<Line> {
    let mut name = String::new();
    let mut params = Vec::new();
    let mut cur = String::new();
    let mut pname: Option<String> = None;
    let mut in_quotes = false;
    let mut value = None;
    for (i, c) in raw.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ';' | ':' if !in_quotes => {
                if name.is_empty() {
                    name = cur.trim().to_ascii_uppercase();
                } else if let Some(p) = pname.take() {
                    params.push((p, std::mem::take(&mut cur)));
                }
                cur.clear();
                if c == ':' {
                    value = Some(raw[i + 1..].to_owned());
                    break;
                }
            }
            '=' if !in_quotes && !name.is_empty() && pname.is_none() => {
                pname = Some(cur.trim().to_ascii_uppercase());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let value = value?;
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    Some(Line {
        name,
        params,
        value,
        raw: raw.to_owned(),
    })
}

fn digits(s: &str, n: usize) -> Option<u32> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    let v = digits(s, 8)?;
    NaiveDate::from_ymd_opt((v / 10_000) as i32, (v / 100) % 100, v % 100)
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    let (h, m, sec) = match s.len() {
        6 => {
            let v = digits(s, 6)?;
            (v / 10_000, (v / 100) % 100, v % 100)
        }
        4 => {
            let v = digits(s, 4)?;
            (v / 100, v % 100, 0)
        }
        _ => return None,
    };
    // Artık saniye (60) bazı üreticilerde görülür; 59'a çekilir.
    NaiveTime::from_hms_opt(h, m, sec.min(59))
}

/// `YYYYMMDD`, `YYYYMMDDTHHMMSS[Z]` değerini çözer.
fn parse_stamp(l: &Line) -> Option<Stamp> {
    let v = l.value.trim();
    let (date_part, time_part) = match v.split_once(['T', 't']) {
        Some((d, t)) => (d, Some(t)),
        None => (v, None),
    };
    let date = parse_date(date_part)?;
    let date_only = l
        .param("VALUE")
        .is_some_and(|p| p.eq_ignore_ascii_case("DATE"));
    let Some(t) = time_part.filter(|_| !date_only) else {
        return Some(Stamp {
            date,
            time: None,
            utc: false,
            tzid: None,
        });
    };
    let utc = t.ends_with(['Z', 'z']);
    let time = parse_time(t.trim_end_matches(['Z', 'z']))?;
    Some(Stamp {
        date,
        time: Some(time),
        utc,
        tzid: l.param("TZID").map(|s| s.trim().to_owned()),
    })
}

/// `P1DT2H30M`, `PT45M`, `P2W`, `-PT1H` → saniye.
fn parse_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let mut secs: i64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            if num.len() > 9 {
                return None;
            }
            continue;
        }
        if c == 'T' {
            in_time = true;
            continue;
        }
        let n: i64 = num.parse().ok()?;
        num.clear();
        secs += match (c, in_time) {
            ('W', false) => n * 7 * 86_400,
            ('D', false) => n * 86_400,
            ('H', true) => n * 3_600,
            ('M', true) => n * 60,
            ('S', true) => n,
            _ => return None,
        };
    }
    if !num.is_empty() {
        return None;
    }
    Some(if neg { -secs } else { secs })
}

/// `+0300`, `-0500`, `+033000` → saniye.
fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let (sign, rest) = match s.as_bytes().first()? {
        b'+' => (1, &s[1..]),
        b'-' => (-1, &s[1..]),
        _ => return None,
    };
    let (h, m, sec) = match rest.len() {
        4 => {
            let v = digits(rest, 4)?;
            (v / 100, v % 100, 0)
        }
        6 => {
            let v = digits(rest, 6)?;
            (v / 10_000, (v / 100) % 100, v % 100)
        }
        _ => return None,
    };
    if h > 23 || m > 59 || sec > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60 + sec) as i32)
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

/// Yalnız `FREQ=YEARLY` kuralları; diğerleri `None` (tek geçiş sayılır).
fn parse_rrule(v: &str, start: NaiveDateTime) -> Option<YearlyRule> {
    let mut yearly = false;
    let mut rule = YearlyRule {
        month: start.month(),
        byday: None,
        monthday: None,
        until: None,
    };
    for part in v.split(';') {
        let Some((k, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => yearly = val.eq_ignore_ascii_case("YEARLY"),
            "BYMONTH" => rule.month = val.split(',').next()?.trim().parse().ok()?,
            "BYMONTHDAY" => rule.monthday = Some(val.split(',').next()?.trim().parse().ok()?),
            "BYDAY" => {
                let d = val.split(',').next()?.trim().to_ascii_uppercase();
                let split = d.len().checked_sub(2)?;
                let (ord, day) = d.split_at(split);
                let ord: i8 = if ord.is_empty() { 1 } else { ord.parse().ok()? };
                rule.byday = Some((ord, weekday(day)?));
            }
            "UNTIL" => {
                let (d, t) = match val.split_once('T') {
                    Some((d, t)) => (d, Some(t)),
                    None => (val, None),
                };
                let date = parse_date(d)?;
                let time = t
                    .and_then(|t| parse_time(t.trim_end_matches('Z')))
                    .unwrap_or(NaiveTime::MIN);
                rule.until = Some(date.and_time(time));
            }
            _ => {}
        }
    }
    if !(1..=12).contains(&rule.month) {
        return None;
    }
    yearly.then_some(rule)
}

impl YearlyRule {
    /// Kuralın `year` yılındaki geçiş günü.
    fn day_in(&self, year: i32) -> Option<NaiveDate> {
        if let Some(d) = self.monthday {
            return NaiveDate::from_ymd_opt(year, self.month, d);
        }
        let (ord, wd) = self.byday?;
        if ord > 0 {
            return NaiveDate::from_weekday_of_month_opt(year, self.month, wd, ord as u8);
        }
        // Ay sonundan geriye: sonraki ayın ilk gününden bir önceki gün.
        let (ny, nm) = if self.month == 12 {
            (year + 1, 1)
        } else {
            (year, self.month + 1)
        };
        let mut day = NaiveDate::from_ymd_opt(ny, nm, 1)?.pred_opt()?;
        while day.weekday() != wd {
            day = day.pred_opt()?;
        }
        for _ in 1..ord.unsigned_abs() {
            day = day.checked_sub_signed(TimeDelta::days(7))?;
        }
        (day.month() == self.month).then_some(day)
    }
}

impl Observance {
    /// `local` anından önceki (ya da o andaki) en son geçiş.
    fn latest_onset_before(&self, local: NaiveDateTime) -> Option<NaiveDateTime> {
        let Some(rule) = &self.rule else {
            return (self.start <= local).then_some(self.start);
        };
        let mut best = None;
        for year in [local.year() - 1, local.year()] {
            if year < self.start.year() {
                continue;
            }
            let Some(day) = rule.day_in(year) else {
                continue;
            };
            let onset = day.and_time(self.start.time());
            if onset < self.start || rule.until.is_some_and(|u| onset > u) || onset > local {
                continue;
            }
            if best.is_none_or(|b| onset > b) {
                best = Some(onset);
            }
        }
        best
    }
}

impl Zone {
    /// `local` duvar saatindeki kayma (saniye).
    fn offset_at(&self, local: NaiveDateTime) -> Option<i32> {
        let mut best: Option<(NaiveDateTime, i32)> = None;
        for o in &self.observances {
            if let Some(onset) = o.latest_onset_before(local) {
                if best.is_none_or(|(b, _)| onset > b) {
                    best = Some((onset, o.to));
                }
            }
        }
        if let Some((_, to)) = best {
            return Some(to);
        }
        // Tüm geçişlerden önce: en eski geçişin öncesindeki kayma.
        self.observances
            .iter()
            .min_by_key(|o| o.start)
            .map(|o| o.from)
    }
}

/// Dilim adındaki açık kayma: "(UTC+03:00) Istanbul", "GMT-5", "UTC".
fn offset_in_name(tz: &str) -> Option<i32> {
    let up = tz.trim().to_ascii_uppercase();
    if matches!(up.as_str(), "UTC" | "GMT" | "Z" | "ETC/UTC" | "ETC/GMT") {
        return Some(0);
    }
    for key in ["UTC", "GMT"] {
        let Some(i) = up.find(key) else {
            continue;
        };
        let rest = &up[i + key.len()..];
        let sign = match rest.as_bytes().first() {
            Some(b'+') => 1,
            Some(b'-') => -1,
            _ => continue,
        };
        let body: String = rest[1..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ':')
            .collect();
        let (h, m) = match body.split_once(':') {
            Some((h, m)) => (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?),
            None if body.len() > 2 => {
                let v: i32 = body.parse().ok()?;
                (v / 100, v % 100)
            }
            None => (body.parse().ok()?, 0),
        };
        if h > 14 || m > 59 {
            return None;
        }
        return Some(sign * (h * 3600 + m * 60));
    }
    None
}

/// Yerel saat dilimindeki duvar saati → UTC milisaniye. Olmayan (geçiş
/// boşluğundaki) ya da iki kez geçen saatte en erken yorum alınır.
fn local_millis(naive: NaiveDateTime) -> Millis {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d.timestamp_millis(),
        LocalResult::None => {
            let off = Local.offset_from_utc_datetime(&naive).local_minus_utc();
            (naive.and_utc().timestamp() - i64::from(off)) * 1000
        }
    }
}

impl Calendar {
    /// Damgayı UTC milisaniyeye çevirir.
    fn resolve(&self, s: &Stamp) -> Option<Millis> {
        let Some(time) = s.time else {
            return Some(s.date.and_time(NaiveTime::MIN).and_utc().timestamp_millis());
        };
        let naive = s.date.and_time(time);
        if s.utc {
            return Some(naive.and_utc().timestamp_millis());
        }
        let Some(tzid) = s.tzid.as_deref() else {
            return Some(local_millis(naive));
        };
        let zone = self.zones.iter().find(|z| {
            z.id.eq_ignore_ascii_case(tzid)
                || z.id.eq_ignore_ascii_case(tzid.trim_start_matches('/'))
        });
        let offset = zone
            .and_then(|z| z.offset_at(naive))
            .or_else(|| offset_in_name(tzid));
        match offset {
            Some(off) => {
                let fixed = FixedOffset::east_opt(off)?;
                Some(
                    fixed
                        .from_local_datetime(&naive)
                        .earliest()?
                        .timestamp_millis(),
                )
            }
            None => Some(local_millis(naive)),
        }
    }
}

/// `mailto:adres` değerini adrese çevirir; CN parametresi görünen addır.
fn address(l: &Line) -> Option<Address> {
    let v = l.value.trim();
    if v.len() < 7 || !v.is_char_boundary(7) || !v[..7].eq_ignore_ascii_case("mailto:") {
        return None;
    }
    let email = v[7..].trim();
    let ok = !email.is_empty()
        && email.chars().count() <= MAX_EMAIL
        && email.contains('@')
        && !email.starts_with('@')
        && !email.ends_with('@')
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | ','));
    if !ok {
        return None;
    }
    let name = l
        .param("CN")
        .map(|n| clean_display_name(&unescape(n)))
        .map(|n| truncate_chars(&n, MAX_TEXT))
        .filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(email));
    Some(Address::new(name.as_deref(), email))
}

impl Event {
    fn apply(&mut self, l: &Line) {
        match l.name.as_str() {
            "UID" => self.uid = text(&l.value),
            "SUMMARY" => self.summary = text(&l.value),
            "LOCATION" => self.location = Some(text(&l.value)).filter(|s| !s.is_empty()),
            "ORGANIZER" => self.organizer = address(l),
            "ATTENDEE" => {
                if self.attendees.len() < MAX_ATTENDEES {
                    if let Some(a) = address(l) {
                        let dup = self
                            .attendees
                            .iter()
                            .any(|x| x.address.email.eq_ignore_ascii_case(&a.email));
                        if !dup {
                            self.attendees.push(Attendee {
                                address: a,
                                partstat: l.param("PARTSTAT").map(|p| p.to_ascii_uppercase()),
                            });
                        }
                    }
                }
            }
            "SEQUENCE" => self.sequence = l.value.trim().parse().unwrap_or(0),
            "DTSTART" => {
                self.start = parse_stamp(l);
                self.raw_start = self.start.is_some().then(|| l.raw.clone());
            }
            "DTEND" => {
                self.end = parse_stamp(l);
                self.raw_end = self.end.is_some().then(|| l.raw.clone());
            }
            "DURATION" => self.duration = parse_duration(&l.value),
            "RECURRENCE-ID" => {
                self.raw_recurrence_id = parse_stamp(l).is_some().then(|| l.raw.clone());
            }
            _ => {}
        }
    }
}

/// Geçiş bileşeni (STANDARD/DAYLIGHT) kurulurken toplanan alanlar.
#[derive(Default)]
struct ObservanceBuilder {
    start: Option<NaiveDateTime>,
    from: Option<i32>,
    to: Option<i32>,
    rrule: Option<String>,
}

impl ObservanceBuilder {
    fn apply(&mut self, l: &Line) {
        match l.name.as_str() {
            "DTSTART" => {
                self.start = parse_stamp(l).and_then(|s| Some(s.date.and_time(s.time?)));
            }
            "TZOFFSETFROM" => self.from = parse_offset(&l.value),
            "TZOFFSETTO" => self.to = parse_offset(&l.value),
            "RRULE" => self.rrule = Some(l.value.clone()),
            _ => {}
        }
    }

    fn finish(self) -> Option<Observance> {
        let start = self.start.unwrap_or_else(|| {
            NaiveDate::from_ymd_opt(1970, 1, 1)
                .expect("geçerli")
                .and_time(NaiveTime::MIN)
        });
        let to = self.to?;
        Some(Observance {
            start,
            to,
            from: self.from.unwrap_or(to),
            rule: self.rrule.as_deref().and_then(|r| parse_rrule(r, start)),
        })
    }
}

/// iCalendar metnini ayrıştırır; `BEGIN:VCALENDAR` yoksa `None`.
pub fn parse_calendar(input: &str) -> Option<Calendar> {
    if input.len() > MAX_ICS {
        return None;
    }
    let mut cal = Calendar::default();
    let mut stack: Vec<String> = Vec::new();
    let mut event: Option<Event> = None;
    let mut zone: Option<Zone> = None;
    let mut obs: Option<ObservanceBuilder> = None;
    let mut seen = false;
    for raw in unfold(input) {
        let Some(l) = parse_line(&raw) else {
            continue;
        };
        match l.name.as_str() {
            "BEGIN" => {
                let comp = l.value.trim().to_ascii_uppercase();
                match comp.as_str() {
                    "VCALENDAR" => seen = true,
                    "VEVENT" if cal.event.is_none() && event.is_none() => {
                        event = Some(Event::default());
                    }
                    "VTIMEZONE" => zone = Some(Zone::default()),
                    "STANDARD" | "DAYLIGHT" if zone.is_some() => {
                        obs = Some(ObservanceBuilder::default());
                    }
                    _ => {}
                }
                if stack.len() < 64 {
                    stack.push(comp);
                }
            }
            "END" => {
                let comp = l.value.trim().to_ascii_uppercase();
                if let Some(pos) = stack.iter().rposition(|c| *c == comp) {
                    stack.truncate(pos);
                }
                match comp.as_str() {
                    "VEVENT" => {
                        if let Some(e) = event.take() {
                            if cal.event.is_none() {
                                cal.event = Some(e);
                            }
                        }
                    }
                    "STANDARD" | "DAYLIGHT" => {
                        if let (Some(b), Some(z)) = (obs.take(), zone.as_mut()) {
                            if let Some(o) = b.finish() {
                                z.observances.push(o);
                            }
                        }
                    }
                    "VTIMEZONE" => {
                        if let Some(z) = zone.take() {
                            if !z.id.is_empty() {
                                cal.zones.push(z);
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => match stack.last().map(String::as_str) {
                Some("VCALENDAR") if l.name == "METHOD" => {
                    cal.method = Some(l.value.trim().to_ascii_uppercase());
                }
                Some("VEVENT") => {
                    if let Some(e) = event.as_mut() {
                        e.apply(&l);
                    }
                }
                Some("VTIMEZONE") if l.name == "TZID" => {
                    if let Some(z) = zone.as_mut() {
                        z.id = truncate_chars(l.value.trim(), MAX_TEXT);
                    }
                }
                Some("STANDARD" | "DAYLIGHT") => {
                    if let Some(b) = obs.as_mut() {
                        b.apply(&l);
                    }
                }
                _ => {}
            },
        }
    }
    // Satır sınırında kesilen ya da END satırı eksik etkinlik yine okunur.
    if cal.event.is_none() {
        cal.event = event;
    }
    seen.then_some(cal)
}

/// Okuma alanı için davet özeti; METHOD REQUEST ya da CANCEL değilse,
/// etkinlik, UID ya da başlangıç yoksa `None`.
pub fn to_invite(cal: &Calendar) -> Option<Invite> {
    let method = match cal.method.as_deref()? {
        "REQUEST" => InviteMethod::Request,
        "CANCEL" => InviteMethod::Cancel,
        _ => return None,
    };
    let e = cal.event.as_ref()?;
    if e.uid.is_empty() {
        return None;
    }
    let start_stamp = e.start.as_ref()?;
    let start = cal.resolve(start_stamp)?;
    let end = match (&e.end, e.duration) {
        (Some(s), _) => cal.resolve(s),
        (None, Some(d)) => start.checked_add(d.checked_mul(1000)?),
        (None, None) => None,
    }
    .filter(|end| *end >= start);
    Some(Invite {
        uid: e.uid.clone(),
        summary: e.summary.clone(),
        start,
        end,
        all_day: start_stamp.time.is_none(),
        location: e.location.clone(),
        organizer: e.organizer.clone(),
        attendees: e.attendees.len() as u32,
        method,
        sequence: e.sequence,
    })
}

/// Postanın takvim parçası (`text/calendar`, `application/ics` ya da `.ics`
/// eki); yoksa ya da `MAX_ICS`'ten büyükse `None`.
pub fn calendar_text(raw: &[u8]) -> Option<String> {
    let msg = MessageParser::default().parse(raw)?;
    for part in &msg.parts {
        let by_type = part.content_type().is_some_and(|ct| {
            let sub = ct.subtype().unwrap_or("");
            (ct.ctype().eq_ignore_ascii_case("text") && sub.eq_ignore_ascii_case("calendar"))
                || (ct.ctype().eq_ignore_ascii_case("application")
                    && sub.eq_ignore_ascii_case("ics"))
        });
        let by_name = part
            .attachment_name()
            .is_some_and(|n| n.to_ascii_lowercase().ends_with(".ics"));
        if !(by_type || by_name) {
            continue;
        }
        let bytes = part.contents();
        if bytes.is_empty() || bytes.len() > MAX_ICS {
            continue;
        }
        let text = String::from_utf8_lossy(bytes);
        if text.contains("BEGIN:VCALENDAR") {
            return Some(text.into_owned());
        }
    }
    None
}

/// Ham postadaki davet (yoksa `None`).
pub fn invite(raw: &[u8]) -> Option<Invite> {
    let cal = parse_calendar(&calendar_text(raw)?)?;
    to_invite(&cal)
}

/// Metin değerini kaçışlar: `\`, `;`, `,`, satır sonu.
fn escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Parametre değeri: tırnak ve denetim karakterleri atılır; ayraç içeriyorsa
/// tırnaklanır.
fn param_value(v: &str) -> String {
    let s: String = v.chars().filter(|c| !c.is_control() && *c != '"').collect();
    if s.contains([';', ',', ':']) {
        format!("\"{s}\"")
    } else {
        s
    }
}

/// Satırı 75 sekizlide katlar (RFC 5545 §3.1), karakter sınırında.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 60);
    let mut count = 0;
    let mut first = true;
    for c in line.chars() {
        let len = c.len_utf8();
        let limit = if first { FOLD_AT } else { FOLD_AT - 1 };
        if count + len > limit {
            out.push_str("\r\n ");
            count = 0;
            first = false;
        }
        out.push(c);
        count += len;
    }
    out
}

fn partstat(response: InviteResponse) -> &'static str {
    match response {
        InviteResponse::Accepted => "ACCEPTED",
        InviteResponse::Tentative => "TENTATIVE",
        InviteResponse::Declined => "DECLINED",
    }
}

/// Davete iMIP yanıtı (METHOD:REPLY). Aynı UID ve SEQUENCE, düzenleyen ve
/// yanıtlayan katılımcı; varsa RECURRENCE-ID, DTSTART ve DTEND aynen döner.
/// Düzenleyen ya da UID yoksa `None`.
pub fn build_reply(
    cal: &Calendar,
    me: &Address,
    response: InviteResponse,
    now: Millis,
) -> Option<String> {
    let e = cal.event.as_ref()?;
    let organizer = e.organizer.as_ref()?;
    if e.uid.is_empty() {
        return None;
    }
    let stamp = Utc
        .timestamp_millis_opt(now)
        .single()?
        .format("%Y%m%dT%H%M%SZ");
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        format!(
            "PRODID:-//Mailhere//Mailhere {}//TR",
            env!("CARGO_PKG_VERSION")
        ),
        "VERSION:2.0".to_owned(),
        "METHOD:REPLY".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{}", escape(&e.uid)),
        format!("SEQUENCE:{}", e.sequence),
        format!("DTSTAMP:{stamp}"),
        mailto_line("ORGANIZER", organizer, None),
        mailto_line("ATTENDEE", me, Some(partstat(response))),
    ];
    for raw in [&e.raw_recurrence_id, &e.raw_start, &e.raw_end]
        .into_iter()
        .flatten()
    {
        lines.push(raw.chars().filter(|c| !c.is_control()).collect());
    }
    lines.push(format!("SUMMARY:{}", escape(&e.summary)));
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    let mut out: String = lines.iter().map(|l| fold(l) + "\r\n").collect();
    out.shrink_to_fit();
    Some(out)
}

fn mailto_line(name: &str, a: &Address, partstat: Option<&str>) -> String {
    let mut s = name.to_owned();
    if let Some(p) = partstat {
        s.push_str(";PARTSTAT=");
        s.push_str(p);
    }
    if let Some(cn) = a.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        s.push_str(";CN=");
        s.push_str(&param_value(cn));
    }
    s.push_str(":mailto:");
    s.push_str(a.email.trim());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE: &str = "BEGIN:VCALENDAR\r
PRODID:-//Ornek//Takvim//TR\r
VERSION:2.0\r
METHOD:REQUEST\r
BEGIN:VTIMEZONE\r
TZID:Europe/Istanbul\r
BEGIN:STANDARD\r
TZOFFSETFROM:+0300\r
TZOFFSETTO:+0300\r
TZNAME:+03\r
DTSTART:19700101T000000\r
END:STANDARD\r
END:VTIMEZONE\r
BEGIN:VEVENT\r
DTSTART;TZID=Europe/Istanbul:20261020T090000\r
DTEND;TZID=Europe/Istanbul:20261020T100000\r
DTSTAMP:20261013T120000Z\r
ORGANIZER;CN=\"Aydın, Kerem\":mailto:kerem.aydin@ornek.com.tr\r
UID:sprint-42@ornek.com.tr\r
ATTENDEE;CUTYPE=INDIVIDUAL;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;CN=Furkan\r
  Yıldırım;X-NUM-GUESTS=0:mailto:furkan@ornek.com.tr\r
ATTENDEE;PARTSTAT=ACCEPTED:mailto:kerem.aydin@ornek.com.tr\r
ATTENDEE;PARTSTAT=NEEDS-ACTION:MAILTO:Murat.Kaya@ornek.com.tr\r
ATTENDEE:mailto:murat.kaya@ornek.com.tr\r
SEQUENCE:2\r
SUMMARY:Sprint planlama\\, 4. çeyrek\r
LOCATION:Toplantı odası 2\\nKat 3\r
BEGIN:VALARM\r
SUMMARY:Hatırlatma\r
TRIGGER:-PT10M\r
END:VALARM\r
END:VEVENT\r
END:VCALENDAR\r
";

    const OUTLOOK: &str = "BEGIN:VCALENDAR
METHOD:REQUEST
PRODID:Ornek Takvim
VERSION:2.0
BEGIN:VTIMEZONE
TZID:W. Europe Standard Time
BEGIN:STANDARD
DTSTART:16010101T030000
TZOFFSETFROM:+0200
TZOFFSETTO:+0100
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
END:STANDARD
BEGIN:DAYLIGHT
DTSTART:16010101T020000
TZOFFSETFROM:+0100
TZOFFSETTO:+0200
RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
END:DAYLIGHT
END:VTIMEZONE
BEGIN:VEVENT
ORGANIZER;CN=Elif Şahin:mailto:elif.sahin@hukukbiz.com.tr
ATTENDEE;ROLE=REQ-PARTICIPANT;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;CN=Furkan:mailto:furkan@ornek.com.tr
SUMMARY;LANGUAGE=tr-TR:Sözleşme görüşmesi
DTSTART;TZID=W. Europe Standard Time:20260715T100000
DTEND;TZID=W. Europe Standard Time:20260715T110000
UID:040000008200E00074C5B7101A82E0080000000012345
SEQUENCE:0
END:VEVENT
END:VCALENDAR
";

    fn cal(s: &str) -> Calendar {
        parse_calendar(s).expect("takvim")
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Millis {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis()
    }

    #[test]
    fn davet_ayristirilir() {
        let c = cal(GOOGLE);
        let i = to_invite(&c).unwrap();
        assert_eq!(i.method, InviteMethod::Request);
        assert_eq!(i.uid, "sprint-42@ornek.com.tr");
        assert_eq!(i.summary, "Sprint planlama, 4. çeyrek");
        assert_eq!(i.location.as_deref(), Some("Toplantı odası 2 Kat 3"));
        assert_eq!(i.start, utc(2026, 10, 20, 6, 0), "+03 → UTC");
        assert_eq!(i.end, Some(utc(2026, 10, 20, 7, 0)));
        assert!(!i.all_day);
        assert_eq!(
            i.organizer,
            Some(Address::new(
                Some("Aydın, Kerem"),
                "kerem.aydin@ornek.com.tr"
            ))
        );
        assert_eq!(i.attendees, 3, "aynı adres bir kez sayılır");
        assert_eq!(i.sequence, 2);
        // VALARM içindeki SUMMARY etkinliği ezmez.
        let e = c.event.as_ref().unwrap();
        assert_eq!(
            e.attendees[0].address.name.as_deref(),
            Some("Furkan Yıldırım")
        );
        assert_eq!(e.attendees[0].partstat.as_deref(), Some("NEEDS-ACTION"));
    }

    #[test]
    fn yaz_kis_saati_kurali() {
        let c = cal(OUTLOOK);
        let i = to_invite(&c).unwrap();
        assert_eq!(i.start, utc(2026, 7, 15, 8, 0), "temmuzda +02:00");
        assert_eq!(i.summary, "Sözleşme görüşmesi");
        let zone = &c.zones[0];
        let at = |y, mo, d, h| {
            zone.offset_at(
                NaiveDate::from_ymd_opt(y, mo, d)
                    .unwrap()
                    .and_hms_opt(h, 0, 0)
                    .unwrap(),
            )
        };
        assert_eq!(at(2026, 12, 15, 10), Some(3600), "aralıkta +01:00");
        assert_eq!(at(2026, 3, 29, 1), Some(3600), "geçiş sabahından önce kış");
        assert_eq!(
            at(2026, 3, 29, 3),
            Some(7200),
            "son pazar 02:00'den sonra yaz"
        );
        assert_eq!(
            at(2026, 10, 25, 4),
            Some(3600),
            "ekim son pazarından sonra kış"
        );
        assert_eq!(
            at(1500, 1, 1, 0),
            Some(3600),
            "tüm geçişlerden önce: en eski geçişin öncesi"
        );
    }

    #[test]
    fn utc_tum_gun_ve_sure() {
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u1\nDTSTART:20261020T090000Z\nDURATION:PT1H30M\nSUMMARY:Z\nEND:VEVENT\nEND:VCALENDAR\n");
        let i = to_invite(&c).unwrap();
        assert_eq!(i.start, utc(2026, 10, 20, 9, 0));
        assert_eq!(i.end, Some(utc(2026, 10, 20, 10, 30)));

        let c = cal("BEGIN:VCALENDAR\nMETHOD:CANCEL\nBEGIN:VEVENT\nUID:u2\nDTSTART;VALUE=DATE:20261020\nDTEND;VALUE=DATE:20261022\nSUMMARY:Tatil\nEND:VEVENT\nEND:VCALENDAR\n");
        let i = to_invite(&c).unwrap();
        assert_eq!(i.method, InviteMethod::Cancel);
        assert!(i.all_day);
        assert_eq!(i.start, utc(2026, 10, 20, 0, 0));
        assert_eq!(i.end, Some(utc(2026, 10, 22, 0, 0)));

        // Bitiş başlangıçtan önceyse atılır.
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u3\nDTSTART:20261020T090000Z\nDTEND:20261020T080000Z\nEND:VEVENT\nEND:VCALENDAR\n");
        assert_eq!(to_invite(&c).unwrap().end, None);
    }

    #[test]
    fn dilimsiz_saat_yerel_sayilir() {
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART:20260115T093000\nEND:VEVENT\nEND:VCALENDAR\n");
        let naive = NaiveDate::from_ymd_opt(2026, 1, 15)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        assert_eq!(to_invite(&c).unwrap().start, local_millis(naive));
        // Bilinmeyen TZID de yerel; adında kayma varsa o.
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART;TZID=Bilinmeyen/Dilim:20260115T093000\nEND:VEVENT\nEND:VCALENDAR\n");
        assert_eq!(to_invite(&c).unwrap().start, local_millis(naive));
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART;TZID=\"(UTC+03:00) Istanbul\":20260115T093000\nEND:VEVENT\nEND:VCALENDAR\n");
        assert_eq!(to_invite(&c).unwrap().start, utc(2026, 1, 15, 6, 30));
        assert_eq!(offset_in_name("GMT-5"), Some(-5 * 3600));
        assert_eq!(offset_in_name("UTC"), Some(0));
        assert_eq!(offset_in_name("Europe/Istanbul"), None);
    }

    #[test]
    fn davet_olmayan_takvimler() {
        assert!(to_invite(&cal("BEGIN:VCALENDAR\nMETHOD:PUBLISH\nBEGIN:VEVENT\nUID:u\nDTSTART:20261020T090000Z\nEND:VEVENT\nEND:VCALENDAR\n")).is_none());
        assert!(
            to_invite(&cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nEND:VCALENDAR\n")).is_none(),
            "etkinlik yok"
        );
        assert!(to_invite(&cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nDTSTART:20261020T090000Z\nEND:VEVENT\nEND:VCALENDAR\n")).is_none(), "UID yok");
        assert!(
            to_invite(&cal(
                "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nEND:VEVENT\nEND:VCALENDAR\n"
            ))
            .is_none(),
            "başlangıç yok"
        );
        assert!(parse_calendar("Merhaba, bu bir takvim değil").is_none());
        assert!(parse_calendar("").is_none());
    }

    #[test]
    fn bozuk_girdi_panik_uretmez_ve_sinirlanir() {
        let junk = "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID\n:yalnız değer\nDTSTART:2026102\nDTSTART:20261399T990000Z\nDTEND;VALUE=DATE:abc\nDURATION:P1X\nSEQUENCE:çok\nORGANIZER:kerem@ornek.com\nORGANIZER:mailto:\nATTENDEE:mailto:a b@c\nRRULE:FREQ=YEARLY\nEND:VEVENT\nEND:VCALENDAR";
        let c = cal(junk);
        let e = c.event.as_ref().unwrap();
        assert!(e.start.is_none() && e.end.is_none() && e.duration.is_none());
        assert_eq!(e.sequence, 0);
        assert!(e.organizer.is_none(), "mailto olmayan düzenleyen alınmaz");
        assert!(e.attendees.is_empty());
        assert!(to_invite(&c).is_none());

        // Çok uzun metinler ve satır sayısı sınırlanır.
        let mut big = String::from(
            "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART:20261020T090000Z\n",
        );
        big.push_str(&format!("SUMMARY:{}\n", "a".repeat(5_000)));
        for i in 0..30_000 {
            big.push_str(&format!("X-DOLGU:{i}\n"));
        }
        big.push_str("LOCATION:sonda\nEND:VEVENT\nEND:VCALENDAR\n");
        let t = std::time::Instant::now();
        let c = cal(&big);
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
        let e = c.event.as_ref().unwrap();
        assert_eq!(e.summary.chars().count(), MAX_TEXT);
        assert!(e.location.is_none(), "satır sınırından sonrası okunmaz");
        assert!(parse_calendar(&"B".repeat(MAX_ICS + 1)).is_none());

        // Katılımcı sayısı sınırı.
        let mut many = String::from(
            "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART:20261020T090000Z\n",
        );
        for i in 0..(MAX_ATTENDEES + 50) {
            many.push_str(&format!("ATTENDEE:mailto:k{i}@ornek.com\n"));
        }
        many.push_str("END:VEVENT\nEND:VCALENDAR\n");
        assert_eq!(
            to_invite(&cal(&many)).unwrap().attendees as usize,
            MAX_ATTENDEES
        );
    }

    #[test]
    fn metin_temizlenir() {
        let c = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART:20261020T090000Z\nSUMMARY:Fatura\u{202E}fdp.exe\\;\\\\ \u{0007}x\nORGANIZER;CN=\"Evil\tX\u{202E}\":mailto:e@ornek.com\nEND:VEVENT\nEND:VCALENDAR\n");
        let i = to_invite(&c).unwrap();
        assert_eq!(i.summary, "Faturafdp.exe;\\ x");
        assert_eq!(i.organizer.unwrap().name.as_deref(), Some("Evil X"));
    }

    #[test]
    fn surekli_ve_kayma_ayristirma() {
        assert_eq!(parse_duration("P1DT2H30M"), Some(86_400 + 9_000));
        assert_eq!(parse_duration("PT45M"), Some(2_700));
        assert_eq!(parse_duration("P2W"), Some(14 * 86_400));
        assert_eq!(parse_duration("-PT1H"), Some(-3_600));
        assert_eq!(parse_duration("PT1H30"), None);
        assert_eq!(parse_duration("P1H"), None, "saat yalnız T'den sonra");
        assert_eq!(parse_offset("+0300"), Some(10_800));
        assert_eq!(parse_offset("-0530"), Some(-19_800));
        assert_eq!(parse_offset("+033000"), Some(12_600));
        assert_eq!(parse_offset("0300"), None);
        assert_eq!(parse_offset("+2500"), None);
        let r = parse_rrule(
            "FREQ=YEARLY;BYMONTH=3;BYDAY=2SU",
            NaiveDate::from_ymd_opt(2007, 1, 1)
                .unwrap()
                .and_hms_opt(2, 0, 0)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(r.day_in(2026), NaiveDate::from_ymd_opt(2026, 3, 8));
        let last = parse_rrule(
            "FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU",
            NaiveDateTime::default(),
        )
        .unwrap();
        assert_eq!(last.day_in(2026), NaiveDate::from_ymd_opt(2026, 10, 25));
        assert!(parse_rrule("FREQ=MONTHLY;BYDAY=1MO", NaiveDateTime::default()).is_none());
    }

    #[test]
    fn yanit_kurulur_ve_geri_okunur() {
        let c = cal(GOOGLE);
        let me = Address::new(Some("Furkan Yıldırım"), "furkan@ornek.com.tr");
        let out = build_reply(&c, &me, InviteResponse::Tentative, 1_792_000_000_000).unwrap();
        // Uzun satırlar katlanır; denetim katlama açılmış metinde.
        let flat = out.replace("\r\n ", "");
        assert!(flat.contains("METHOD:REPLY\r\n"));
        assert!(flat.contains("UID:sprint-42@ornek.com.tr\r\n"));
        assert!(flat.contains("SEQUENCE:2\r\n"));
        assert!(
            flat.contains(
                "ATTENDEE;PARTSTAT=TENTATIVE;CN=Furkan Yıldırım:mailto:furkan@ornek.com.tr\r\n"
            ),
            "{flat}"
        );
        assert!(flat.contains("ORGANIZER;CN=\"Aydın, Kerem\":mailto:kerem.aydin@ornek.com.tr\r\n"));
        assert!(flat.contains("DTSTART;TZID=Europe/Istanbul:20261020T090000\r\n"));
        assert!(flat.contains("SUMMARY:Sprint planlama\\, 4. çeyrek\r\n"));
        assert!(flat.contains("DTSTAMP:20261014T"), "{flat}");
        for line in out.split("\r\n") {
            assert!(line.len() <= FOLD_AT, "{line}");
        }
        let back = cal(&out);
        assert_eq!(back.method.as_deref(), Some("REPLY"));
        let e = back.event.unwrap();
        assert_eq!(e.uid, "sprint-42@ornek.com.tr");
        assert_eq!(e.sequence, 2);
        assert_eq!(e.attendees.len(), 1);
        assert_eq!(e.attendees[0].partstat.as_deref(), Some("TENTATIVE"));
        assert_eq!(e.organizer.unwrap().email, "kerem.aydin@ornek.com.tr");

        // Düzenleyen yoksa yanıt kurulmaz.
        let no_org = cal("BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:u\nDTSTART:20261020T090000Z\nEND:VEVENT\nEND:VCALENDAR\n");
        assert!(build_reply(&no_org, &me, InviteResponse::Accepted, 0).is_none());
    }

    #[test]
    fn uzun_satir_katlanir() {
        let long = format!("SUMMARY:{}", "ğ".repeat(100));
        let folded = fold(&long);
        for piece in folded.split("\r\n") {
            assert!(piece.len() <= FOLD_AT, "{}", piece.len());
        }
        assert_eq!(folded.replace("\r\n ", ""), long);
    }

    #[test]
    fn postadaki_takvim_parcasi_bulunur() {
        let ics = GOOGLE;
        let msg = format!(
            "From: Kerem <kerem.aydin@ornek.com.tr>\r\nTo: furkan@ornek.com.tr\r\nSubject: Davet: Sprint planlama\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSprint planlama toplantısına davetlisin.\r\n--b\r\nContent-Type: text/calendar; charset=UTF-8; method=REQUEST\r\n\r\n{ics}--b--\r\n"
        );
        let i = invite(msg.as_bytes()).unwrap();
        assert_eq!(i.summary, "Sprint planlama, 4. çeyrek");
        assert!(calendar_text(msg.as_bytes())
            .unwrap()
            .starts_with("BEGIN:VCALENDAR"));

        // .ics eki (application/octet-stream) de bulunur.
        let b64 = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.encode(GOOGLE.as_bytes())
        };
        let att = format!(
            "From: a@ornek.com\r\nSubject: x\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n--m\r\nContent-Type: text/plain\r\n\r\nEkte.\r\n--m\r\nContent-Type: application/octet-stream; name=\"davet.ics\"\r\nContent-Disposition: attachment; filename=\"davet.ics\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{b64}\r\n--m--\r\n"
        );
        assert_eq!(
            invite(att.as_bytes()).unwrap().uid,
            "sprint-42@ornek.com.tr"
        );

        let plain = "From: a@ornek.com\r\nSubject: x\r\n\r\nMerhaba";
        assert!(invite(plain.as_bytes()).is_none());
        assert!(invite(b"").is_none());
    }
}
