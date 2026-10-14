//! QR matrisi: kod metninden arayüzün çizeceği modül ızgarası.
//!
//! Hata düzeltme M (%15): ekran fotoğrafı yeterli, kod ~200 karakterde
//! sürüm 10 civarı (57×57) kalır. Matris `QrMatrix { size, rows }` olarak
//! verilir; her satır `'1'`/`'0'` dizgisidir. Sessiz bölge dahil değildir.

use mh_core::QrMatrix;
use qrcode::{Color, EcLevel, QrCode};

use crate::error::{TransferError, TransferResult};

/// Metni QR matrisine çevirir.
pub fn matrix(text: &str) -> TransferResult<QrMatrix> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).map_err(|_| {
        TransferError::Internal(
            "QR kod oluşturulamadı: kod metni QR'a sığmadı. Daha az ağ adresiyle yeniden dene."
                .into(),
        )
    })?;
    let size = code.width();
    let rows = code
        .to_colors()
        .chunks(size)
        .map(|row| {
            row.iter()
                .map(|c| if *c == Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect();
    Ok(QrMatrix {
        size: size as u32,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_matrisi_uretilir() {
        let text = "mailhere://aktar?v=1&h=192.168.1.20,10.8.0.2,fd12:3456::20,172.16.5.4,100.64.3.9,10.1.1.1&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
        let m = matrix(text).unwrap();
        // QR boyutu 21 + 4·(sürüm-1); kare ve her satır yalnız 0/1.
        assert!(
            m.size >= 21 && (m.size - 21).is_multiple_of(4),
            "{}",
            m.size
        );
        assert!(
            m.size <= 77,
            "altı adresli kod sürüm 15'i aşmamalı: {}",
            m.size
        );
        assert_eq!(m.rows.len() as u32, m.size);
        for row in &m.rows {
            assert_eq!(row.chars().count() as u32, m.size);
            assert!(row.chars().all(|c| c == '0' || c == '1'));
        }
        // Sol üst bulucu deseni: 7×7, dış çerçeve koyu.
        assert!(m.rows[0].starts_with("1111111"));
        assert!(m.rows[6].starts_with("1111111"));
        assert!(m.rows[1].starts_with("1000001"));
        // Aynı metin aynı matris.
        assert_eq!(matrix(text).unwrap(), m);
        // Sığmayan metin anlamlı hata.
        assert!(matrix(&"a".repeat(5000)).is_err());
    }
}
