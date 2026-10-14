//! Yerel ağ adresleri: kodda duyurulacak adreslerin seçimi ve bağlanan eşin
//! denetimi.
//!
//! Kodda duyurulan adresler (`select_hosts`): özel IPv4 (10/8, 172.16/12,
//! 192.168/16), CGNAT 100.64/10 ve IPv6 ULA (fc00::/7). Kararlar:
//! - **100.64/10 dahil.** Bu blok taşıyıcı NAT'ının yanı sıra Tailscale gibi
//!   örtü ağlarında kullanılır; telefon ve bilgisayar aynı örtü ağdaysa en
//!   kolay yol budur. Blok zaten genel internet sayılmaz (`is_public_ip`
//!   false), eş denetimiyle de uyumludur. Kötüye kullanım için K gerekir.
//! - **IPv6 link-local (fe80::/10) dahil değil.** Bu adrese bağlanmak için
//!   bağlanan tarafın kendi arayüz adı (zone) gerekir; kodda taşınamaz.
//!   IPv4 link-local (169.254/16) da dahil değil: DHCP'siz nadir durum
//!   dışında işe yaramaz ve Windows'ta kopuk adaptörlerde sık görülür.
//! - **Loopback asla** (testlerde `allow_loopback` ile açılır), genel asla.
//!
//! Eş denetimi (`peer_allowed`) daha geniştir: bağlanan telefonun adresi
//! yukarıdakilere ek olarak link-local olabilir (aynı bağda doğrudan
//! bağlantı). Genel adres, çoklu yayın, belirtilmemiş ve ayrılmış bloklar
//! her zaman reddedilir.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use mh_core::net::is_public_ip;

/// Kodda en çok bu kadar adres duyurulur.
pub const MAX_HOSTS: usize = 6;

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (o[1] & 0xc0) == 64
}

fn is_ula(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xfe00) == 0xfc00
}

fn is_v6_link_local(ip: Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Adres yerel bir ağa mı ait: özel IPv4, CGNAT, IPv4/IPv6 link-local ya da
/// IPv6 ULA. IPv4 eşlenmiş IPv6 adresleri IPv4 olarak değerlendirilir.
/// Loopback, genel, çoklu yayın, belirtilmemiş ve ayrılmış bloklar değildir.
pub fn is_local_network(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.is_private() || is_cgnat(v4) || v4.is_link_local(),
        IpAddr::V6(v6) => is_ula(v6) || is_v6_link_local(v6),
    }
}

/// Bağlanan eş kabul edilir mi? Yalnız yerel ağ adresleri; loopback yalnız
/// `allow_loopback` ile (testler). Genel adres asla.
pub fn peer_allowed(ip: IpAddr, allow_loopback: bool) -> bool {
    let ip = ip.to_canonical();
    if ip.is_loopback() {
        return allow_loopback;
    }
    !is_public_ip(ip) && is_local_network(ip)
}

/// Kodda duyurulabilir mi: özel IPv4, CGNAT ya da IPv6 ULA (link-local değil).
fn advertisable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || is_cgnat(v4),
        IpAddr::V6(v6) => is_ula(v6),
    }
}

/// Bir arayüz adresi adayı (`local_hosts` bunları `if-addrs`'tan derler;
/// testler elle kurar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Arayüz adı ("wlan0", "Wi-Fi", "docker0").
    pub name: String,
    pub ip: IpAddr,
    /// Arayüz çalışıyor (bilinmiyorsa `true` verilir).
    pub up: bool,
    /// Noktadan noktaya (VPN tüneli).
    pub p2p: bool,
}

/// Arayüz adına göre öncelik: 0 fiziksel, 1 VPN/tünel, 2 sanal köprü ve
/// konteyner ağları. Yalnız sıralamayı etkiler; hiçbir adres bu yüzden atılmaz.
fn rank(c: &Candidate) -> u8 {
    let n = c.name.to_ascii_lowercase();
    const VIRTUAL: [&str; 12] = [
        "docker",
        "br-",
        "veth",
        "virbr",
        "vmnet",
        "vboxnet",
        "vethernet",
        "lxc",
        "lxd",
        "cni",
        "flannel",
        "podman",
    ];
    const TUNNEL: [&str; 7] = ["tun", "tap", "wg", "utun", "ppp", "tailscale", "zt"];
    if VIRTUAL.iter().any(|p| n.starts_with(p)) {
        2
    } else if c.p2p || TUNNEL.iter().any(|p| n.starts_with(p)) {
        1
    } else {
        0
    }
}

/// Adaylardan kodda duyurulacak adresleri seçer: çalışmayan arayüzler ve
/// duyurulamaz adresler atılır, yinelenenler birleştirilir; fiziksel
/// arayüzler ve IPv4 öne alınır; en çok [`MAX_HOSTS`] adres.
pub fn select_hosts(
    cands: impl IntoIterator<Item = Candidate>,
    allow_loopback: bool,
) -> Vec<IpAddr> {
    let mut picked: Vec<(u8, bool, usize, IpAddr)> = Vec::new();
    for (i, c) in cands.into_iter().enumerate() {
        if !c.up {
            continue;
        }
        let ip = c.ip.to_canonical();
        let ok = if ip.is_loopback() {
            allow_loopback
        } else {
            !is_public_ip(ip) && advertisable(ip)
        };
        if !ok || picked.iter().any(|p| p.3 == ip) {
            continue;
        }
        picked.push((rank(&c), ip.is_ipv6(), i, ip));
    }
    picked.sort_by_key(|p| (p.0, p.1, p.2));
    picked.into_iter().map(|p| p.3).take(MAX_HOSTS).collect()
}

/// Bu bilgisayarın kodda duyurulacak adresleri. Arayüzler okunamazsa boş.
pub fn local_hosts(allow_loopback: bool) -> Vec<IpAddr> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "ağ arayüzleri okunamadı");
            return Vec::new();
        }
    };
    select_hosts(
        ifaces.into_iter().map(|i| Candidate {
            up: !matches!(
                i.oper_status,
                if_addrs::IfOperStatus::Down
                    | if_addrs::IfOperStatus::NotPresent
                    | if_addrs::IfOperStatus::LowerLayerDown
            ),
            p2p: i.is_p2p,
            ip: i.ip(),
            name: i.name,
        }),
        allow_loopback,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn cand(name: &str, addr: &str) -> Candidate {
        Candidate {
            name: name.into(),
            ip: ip(addr),
            up: true,
            p2p: false,
        }
    }

    #[test]
    fn genel_adresten_es_reddedilir() {
        for bad in [
            "8.8.8.8",
            "93.184.216.34",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
            "0.0.0.0",
            "::",
            "224.0.0.1",
            "ff02::1",
            "255.255.255.255",
            "192.0.2.1",   // belgeleme: yerel ağ değil
            "198.18.0.1",  // ölçüm
            "2001:db8::1", // belgeleme
            "64:ff9b::808:808",
        ] {
            assert!(!peer_allowed(ip(bad), false), "{bad}");
            assert!(!peer_allowed(ip(bad), true), "{bad} (loopback serbestken)");
        }
        for good in [
            "192.168.1.4",
            "10.8.0.2",
            "172.16.5.4",
            "100.64.3.9",
            "169.254.10.1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.1.4",
        ] {
            assert!(peer_allowed(ip(good), false), "{good}");
        }
        // Loopback yalnız açıkça izin verildiğinde.
        assert!(!peer_allowed(ip("127.0.0.1"), false));
        assert!(!peer_allowed(ip("::1"), false));
        assert!(!peer_allowed(ip("::ffff:127.0.0.1"), false));
        assert!(peer_allowed(ip("127.0.0.1"), true));
        assert!(peer_allowed(ip("::1"), true));
        assert!(peer_allowed(ip("::ffff:127.0.0.1"), true));
    }

    #[test]
    fn yerel_adresler_genel_icermez() {
        // Gerçek arayüzler: ne çıkarsa çıksın genel, loopback, belirtilmemiş,
        // çoklu yayın ya da link-local olmaz; en çok 6 adres.
        let hosts = local_hosts(false);
        assert!(hosts.len() <= MAX_HOSTS);
        for h in &hosts {
            assert!(!is_public_ip(*h), "{h}");
            assert!(
                !h.is_loopback() && !h.is_unspecified() && !h.is_multicast(),
                "{h}"
            );
            assert!(is_local_network(*h), "{h}");
            match h {
                IpAddr::V4(v4) => assert!(!v4.is_link_local(), "{h}"),
                IpAddr::V6(v6) => assert!(!is_v6_link_local(*v6), "{h}"),
            }
        }
        // Loopback'e izin verilince yalnız loopback eklenebilir.
        for h in local_hosts(true) {
            assert!(!is_public_ip(h), "{h}");
        }

        // Sentetik aday listesi: süzme, sıralama, yineleme ve üst sınır.
        let cands = vec![
            cand("lo", "127.0.0.1"),
            cand("eth0", "93.184.216.34"),    // genel: atılır
            cand("eth0", "169.254.7.7"),      // IPv4 link-local: duyurulmaz
            cand("docker0", "172.17.0.1"),    // sanal: sona
            cand("wlan0", "192.168.1.20"),    // fiziksel IPv4: başa
            cand("wlan0", "fe80::1"),         // IPv6 link-local: duyurulmaz
            cand("wlan0", "fd12:3456::20"),   // ULA: IPv4'lerden sonra
            cand("wlan0", "2a00:1450::1"),    // genel IPv6: atılır
            cand("tailscale0", "100.64.3.9"), // CGNAT/örtü: tünel sırası
            cand("wlan0", "192.168.1.20"),    // yineleme
            Candidate {
                name: "eth1".into(),
                ip: ip("10.0.0.5"),
                up: false,
                p2p: false,
            }, // kapalı arayüz: atılır
            cand("eth2", "10.1.1.1"),
            cand("eth3", "10.1.1.2"),
            cand("eth4", "10.1.1.3"),
            cand("eth5", "10.1.1.4"),
        ];
        let hosts = select_hosts(cands.clone(), false);
        assert_eq!(
            hosts,
            vec![
                ip("192.168.1.20"),
                ip("10.1.1.1"),
                ip("10.1.1.2"),
                ip("10.1.1.3"),
                ip("10.1.1.4"),
                ip("fd12:3456::20"),
            ]
        );
        assert_eq!(hosts.len(), MAX_HOSTS);
        // Daha az adayla sıra: fiziksel v4, fiziksel v6, tünel, sanal.
        let few = select_hosts(cands[..9].to_vec(), false);
        assert_eq!(
            few,
            vec![
                ip("192.168.1.20"),
                ip("fd12:3456::20"),
                ip("100.64.3.9"),
                ip("172.17.0.1"),
            ]
        );
        // Testlerde loopback açıkça istenirse listeye girer.
        let with_lo = select_hosts(vec![cand("lo", "127.0.0.1")], true);
        assert_eq!(with_lo, vec![ip("127.0.0.1")]);
        assert!(select_hosts(vec![cand("lo", "127.0.0.1")], false).is_empty());
        // IPv4 eşlenmiş IPv6 adresi IPv4 olarak duyurulur.
        assert_eq!(
            select_hosts(vec![cand("x", "::ffff:192.168.1.9")], false),
            vec![ip("192.168.1.9")]
        );
    }

    #[test]
    fn yerel_ag_siniflari() {
        assert!(is_local_network(ip("10.0.0.1")));
        assert!(is_local_network(ip("100.127.0.1")));
        assert!(is_local_network(ip("169.254.1.1")));
        assert!(is_local_network(ip("fc00::1")));
        assert!(is_local_network(ip("fe80::abcd")));
        assert!(!is_local_network(ip("100.128.0.1")));
        assert!(!is_local_network(ip("127.0.0.1")));
        assert!(!is_local_network(ip("8.8.8.8")));
        assert!(!is_local_network(ip("2001:db8::1")));
        assert!(!is_local_network(ip("fec0::1")));
    }
}
