//! DNS SRV kayıtları (RFC 6186 / RFC 8314).
//!
//! Yanıt DNSSEC ile doğrulanmaz: hedef e-posta alan adı dışındaysa sonuç
//! `other_domain` ile işaretlenir (bkz. `super::discover`), arayüz uyarır.

use std::time::Duration;

use hickory_resolver::proto::rr::RData;
use hickory_resolver::TokioResolver;
use mh_core::{Security, ServerConfig};

const TIMEOUT: Duration = Duration::from_secs(5);

async fn best(resolver: &TokioResolver, name: String) -> Option<(String, u16)> {
    let lookup = tokio::time::timeout(TIMEOUT, resolver.srv_lookup(name))
        .await
        .ok()?
        .ok()?;
    let mut records: Vec<_> = lookup
        .answers()
        .iter()
        .filter_map(|r| match &r.data {
            RData::SRV(srv) => Some(srv.clone()),
            _ => None,
        })
        // "." hedefi hizmetin bilerek kapalı olduğunu söyler.
        .filter(|s| !s.target.is_root() && s.port != 0)
        .collect();
    records.sort_by_key(|s| (s.priority, std::cmp::Reverse(s.weight)));
    let s = records.first()?;
    // ASCII (punycode) biçim: alan adı karşılaştırması ve bağlantı için.
    let host = s.target.to_ascii();
    Some((host.trim_end_matches('.').to_owned(), s.port))
}

pub async fn lookup(domain: &str, email: &str) -> Option<(ServerConfig, ServerConfig)> {
    let resolver = TokioResolver::builder_tokio().ok()?.build().ok()?;
    let q = |svc: &str| format!("{svc}._tcp.{domain}.");
    let (imaps, imap, subs, sub) = tokio::join!(
        best(&resolver, q("_imaps")),
        best(&resolver, q("_imap")),
        best(&resolver, q("_submissions")),
        best(&resolver, q("_submission")),
    );
    let cfg = |(host, port): (String, u16), security| ServerConfig {
        host,
        port,
        security,
        username: email.to_owned(),
    };
    let imap = imaps
        .map(|h| cfg(h, Security::Tls))
        .or_else(|| imap.map(|h| cfg(h, Security::Starttls)))?;
    let smtp = subs
        .map(|h| cfg(h, Security::Tls))
        .or_else(|| sub.map(|h| cfg(h, Security::Starttls)))?;
    Some((imap, smtp))
}
