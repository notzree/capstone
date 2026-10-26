//! Native CSI interpretation. Unsupported layouts remain raw, never guessed from length alone.
//!
//! Original ESP32 table: ESP-IDF v5.3.1 Wi-Fi CSI guide, linked in EXPORT.md.
//! Each complex entry is signed [imaginary, real]. LTF order is LLTF, HT-LTF,
//! STBC-HT-LTF; each HT20 block is indexed 0..31, -32..-1.

use csi_protocol::{Chip, Csi, Hello};

pub const DECODER: &str = "esp32-ht20/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Samples {
    pub real: Vec<i8>,
    pub imag: Vec<i8>,
    pub ltf: Vec<&'static str>,
    pub subcarrier: Vec<i16>,
    /// False for null bins, hardware-invalid words and unsuccessful RX records.
    pub valid: Vec<bool>,
    /// Pilot bins are retained as valid channel observations, separately identified.
    pub pilot: Vec<bool>,
}

/// Deliberately restrict the first decoder to original ESP32, 20 MHz, no secondary
/// channel and no LTF merge. Other combinations need their own verified table.
pub fn decode(hello: Option<&Hello>, csi: &Csi<'_>) -> Result<Samples, &'static str> {
    let hello = hello.ok_or("missing_hello")?;
    if hello.chip != Chip::Esp32 {
        return Err("unsupported_chip");
    }
    let config = hello.csi_config;
    if csi.rx.cwb != 0 || csi.rx.secondary_channel != 0 || config.ltf_merge {
        return Err("unsupported_bandwidth_or_merge");
    }
    if csi.rx.sig_mode > 1 || csi.rx.stbc > 1 {
        return Err("unsupported_phy");
    }
    let mut blocks = Vec::new();
    if config.lltf {
        blocks.push(("lltf", 26));
    }
    if csi.rx.sig_mode == 1 {
        if config.htltf {
            blocks.push(("htltf", 28));
        }
        if csi.rx.stbc == 1 && config.stbc_htltf2 {
            blocks.push(("stbc_htltf2", 28));
        }
    }
    if blocks.is_empty() || csi.data.len() != blocks.len() * 128 {
        return Err("unexpected_length");
    }
    let mut out = Samples {
        real: Vec::new(),
        imag: Vec::new(),
        ltf: Vec::new(),
        subcarrier: Vec::new(),
        valid: Vec::new(),
        pilot: Vec::new(),
    };
    for (block, &(ltf, edge)) in blocks.iter().enumerate() {
        for bin in 0..64 {
            let offset = block * 128 + bin * 2;
            let k = if bin < 32 {
                bin as i16
            } else {
                bin as i16 - 64
            };
            out.imag.push(csi.data[offset] as i8);
            out.real.push(csi.data[offset + 1] as i8);
            out.ltf.push(ltf);
            out.subcarrier.push(k);
            out.valid.push(
                k != 0
                    && k.abs() <= edge
                    && !(csi.first_word_invalid && offset < 4)
                    && csi.rx.rx_state == 0,
            );
            out.pilot.push(matches!(k, -21 | -7 | 7 | 21));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use csi_protocol::{CsiConfig, Roles, RxMeta};

    #[test]
    fn golden_signed_iq_ltf_indices_and_masks() {
        let hello = Hello {
            chip: Chip::Esp32,
            roles: Roles {
                tx: false,
                rx: true,
            },
            firmware_version: [0, 2, 2],
            firmware_commit: [0; 8],
            channel: 6,
            secondary_channel: 0,
            pool_capacity: 32,
            csi_config: CsiConfig {
                lltf: true,
                htltf: true,
                ..Default::default()
            },
        };
        // Hand-written anchors, including signed endpoints and the LTF boundary.
        let mut bytes = [0u8; 256];
        bytes[..6].copy_from_slice(&[0x80, 0x7f, 0xff, 0x01, 0x02, 0xfe]);
        bytes[126..132].copy_from_slice(&[0xfd, 0x04, 0x05, 0xfa, 0xf9, 0x08]);
        let csi = Csi {
            capture_seq: 0,
            node_time_us: 0,
            rx_timestamp_us: 0,
            source_mac: [0; 6],
            probe: None,
            first_word_invalid: true,
            rx: RxMeta {
                sig_mode: 1,
                ..Default::default()
            },
            data: &bytes,
        };
        let s = decode(Some(&hello), &csi).unwrap();
        assert_eq!(&s.imag[..3], &[-128, -1, 2]);
        assert_eq!(&s.real[..3], &[127, 1, -2]);
        assert_eq!(&s.imag[63..66], &[-3, 5, -7]);
        assert_eq!(&s.real[63..66], &[4, -6, 8]);
        assert_eq!(&s.subcarrier[30..34], &[30, 31, -32, -31]);
        assert_eq!(&s.subcarrier[63..66], &[-1, 0, 1]);
        assert_eq!(&s.ltf[63..66], &["lltf", "htltf", "htltf"]);
        assert!(!s.valid[0] && !s.valid[1] && s.valid[2]);
        assert!(!s.valid[27] && s.valid[26] && s.valid[64 + 28]);
        assert!(!s.valid[64] && !s.valid[64 + 29]);
        assert!(s.pilot[7] && s.pilot[43] && !s.pilot[8]);
        assert_eq!(s.valid.iter().filter(|&&v| v).count(), 107);
        assert_eq!(decode(None, &csi), Err("missing_hello"));
        assert_eq!(
            decode(
                Some(&hello),
                &Csi {
                    data: &bytes[..128],
                    ..csi
                }
            ),
            Err("unexpected_length")
        );
        assert_eq!(
            decode(
                Some(&Hello {
                    chip: Chip::Esp32S3,
                    ..hello
                }),
                &csi
            ),
            Err("unsupported_chip")
        );
    }
}
