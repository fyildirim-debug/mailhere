//! Ağ adresi sınıflandırması: bir IP genel internette mi?
//!
//! Abonelikten çıkma isteği (SSRF koruması) ve QR ile ayar aktarımı (yalnız
//! yerel ağ eşleri) aynı denetimi kullanır; bu yüzden ortak tiplerin
//! yanındadır. Yalnız standart kitaplığa dayanır.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// IPv4 adresi genel internette mi? Loopback, özel (10/8, 172.16/12,
/// 192.168/16), link-local, CGNAT (100.64/10), IETF/belgeleme/ölçüm blokları,
/// çoklu yayın, ayrılmış (240/4) ve "bu ağ" (0/8) değildir.
pub fn is_public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(o[0] == 0 // "bu ağ" ve belirtilmemiş (0.0.0.0/8)
        || ip.is_loopback() // 127.0.0.0/8
        || ip.is_private() // 10/8, 172.16/12, 192.168/16
        || ip.is_link_local() // 169.254/16
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF 192.0.0/24
        || ip.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || (o[0] == 198 && (o[1] & 0xfe) == 18) // ölçüm 198.18/15
        || ip.is_multicast() // 224/4
        || o[0] >= 240) // ayrılmış 240/4 ve yayın
}

/// IPv6 adresi genel internette mi? IPv4 taşıyan biçimler (eşlenmiş,
/// NAT64, 6to4) gömülü IPv4 adresine göre değerlendirilir; ULA, link-local,
/// site-local, çoklu yayın, belirtilmemiş, loopback, IETF/belgeleme ve
/// "atılacak" blokları değildir.
pub fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    let embedded =
        |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // IPv4 taşıyan biçimler gömülü adrese göre değerlendirilir.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        return is_public_v4(embedded(s[6], s[7])); // NAT64 64:ff9b::/96
    }
    if s[0] == 0x2002 {
        return is_public_v4(embedded(s[1], s[2])); // 6to4 2002::/16
    }
    !(s[0..6] == [0, 0, 0, 0, 0, 0] // ::/96: belirtilmemiş, loopback, eski IPv4 uyumlu
        || ip.is_multicast() // ff00::/8
        || (s[0] & 0xfe00) == 0xfc00 // ULA fc00::/7
        || (s[0] & 0xffc0) == 0xfe80 // link-local fe80::/10
        || (s[0] & 0xffc0) == 0xfec0 // eski site-local fec0::/10
        || (s[0] == 0x2001 && s[1] < 0x0200) // IETF 2001::/23 (Teredo dahil)
        || (s[0] == 0x2001 && s[1] == 0x0db8) // belgeleme 2001:db8::/32
        || (s[0] == 0x0100 && s[1..4] == [0, 0, 0])) // atılacak 100::/64
}

/// Adres genel internette mi? Loopback, özel, link-local, ULA, CGNAT,
/// çoklu yayın, belirtilmemiş, belgeleme ve ayrılmış bloklar değildir.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genel_adres_denetimi() {
        for bad in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "127.255.0.9",
            "10.0.0.1",
            "172.16.5.4",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.254",
            "192.0.0.8",
            "192.0.2.1",
            "198.18.0.1",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.251",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
            "64:ff9b::a00:1",
            "2002:c0a8:101::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001::1",
            "2001:db8::1",
            "100::1",
        ] {
            assert!(!is_public_ip(bad.parse().unwrap()), "{bad}");
        }
        for good in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.169.0.1",
            "2606:4700:4700::1111",
            "2a00:1450:4001::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
            "2001:4860:4860::8888",
        ] {
            assert!(is_public_ip(good.parse().unwrap()), "{good}");
        }
    }

    #[test]
    fn v4_ve_v6_dogrudan() {
        assert!(is_public_v4(Ipv4Addr::new(9, 9, 9, 9)));
        assert!(!is_public_v4(Ipv4Addr::new(192, 168, 0, 1)));
        assert!(is_public_v6("2606:4700::1".parse().unwrap()));
        assert!(!is_public_v6("fd00::1".parse().unwrap()));
    }
}
