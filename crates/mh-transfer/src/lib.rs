//! Mailhere: bilgisayardan telefona **tek QR kodla** ayar aktarımı.
//!
//! Bu crate yalnız kripto, tel protokolü, yerel ağ adresleri, QR matrisi ve
//! tokio tabanlı sunucu/istemciyi içerir. Depoyu, Tauri'yi ve paketin
//! içeriğini bilmez: paket çağıranın verdiği baytlardır (mh-app JSON kurar
//! ve çözer).
//!
//! # Akış
//! 1. Bilgisayar: [`server::start`] → yerel kapı + kod + QR; durumlar
//!    `waiting → connected → delivered → applied` (ya da `failed` /
//!    `expired` / `cancelled`) bir kanaldan gelir.
//! 2. Telefon: QR'daki metni [`Code::parse`] ile çözer, [`client::fetch`]
//!    ile paketi alır (K ağdan geçmez; paket AES-256-GCM ile gider), uygular
//!    ve [`client::confirm`] ile sonucu bildirir.
//!
//! # Güvenlik özeti
//! - Kök anahtar K yalnız QR'da; ağda yalnız T (teklif kimliği) açık gider.
//! - Selam HMAC-SHA256(k_auth) ile doğrulanır; hatalı selam yanıtsız kapanır
//!   ve sayılır (5 → teklif kapanır). Paket tek bir istemciye bir kez verilir.
//! - Sunucu yalnız yerel ağ eşlerini kabul eder; kod yalnız yerel ağ
//!   adresleri taşır; genel adres içeren kod reddedilir.
//! - Süreler: teklif 180 s, onay 8 dk, okuma 10 s, istemci toplam 30 s.
//! - Gizli değerler (`K`, türetilmiş anahtarlar, düz paket) `Zeroizing`;
//!   `Debug` çıktıları ve hata mesajları gizli değer içermez.
//!
//! Bayt düzeni için [`crypto`] modülünün belgesine bak.

pub mod addr;
pub mod client;
pub mod code;
pub mod crypto;
mod error;
pub mod qr;
pub mod server;

pub use addr::{is_local_network, local_hosts, peer_allowed, select_hosts, Candidate};
pub use client::{confirm, fetch, ClientOptions};
pub use code::{Code, MAX_CODE_CHARS, VERSION};
pub use crypto::MAX_PACKAGE;
pub use error::{TransferError, TransferResult};
pub use qr::matrix as qr_matrix;
pub use server::{start, Offer, OfferHandle, OfferOptions, MAX_CONNECTIONS};

/// Telefonun onay mesajındaki sonuç (`durum u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ApplyStatus {
    /// Her şey uygulandı.
    Applied = 0,
    /// Uygulandı ama bazı öğeler atlandı (parolasız hesap, hesabı olmayan kural…).
    Partial = 1,
    /// Paket uygulanamadı.
    Failed = 2,
}

impl ApplyStatus {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Tel üzerindeki bayttan; bilinmeyen değer `None`.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Applied),
            1 => Some(Self::Partial),
            2 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onay_durumu_baytlari() {
        for s in [
            ApplyStatus::Applied,
            ApplyStatus::Partial,
            ApplyStatus::Failed,
        ] {
            assert_eq!(ApplyStatus::from_u8(s.as_u8()), Some(s));
        }
        assert_eq!(ApplyStatus::from_u8(3), None);
        assert_eq!(ApplyStatus::from_u8(255), None);
        assert_eq!(ApplyStatus::Applied.as_u8(), 0);
    }
}
