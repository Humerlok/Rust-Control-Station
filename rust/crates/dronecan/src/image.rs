//! Firmware images for DroneCAN nodes: the application descriptor that ArduPilot peripheral
//! firmware embeds, which carries the image CRC the node reports in `GetNodeInfo`.

/// The two signatures that start an AP_Periph application descriptor.
const DESCRIPTOR_SIGNATURES: [[u8; 8]; 2] = [
    [0xD7, 0xE4, 0xF7, 0xBA, 0xD0, 0x0F, 0x9B, 0xEE],
    [0x40, 0xA2, 0xE4, 0xF1, 0x64, 0x68, 0x91, 0x06],
];

/// The 64-bit image CRC stored right after the descriptor signature (little-endian), or `None`
/// when the image carries no descriptor.
pub fn app_descriptor_crc(image: &[u8]) -> Option<u64> {
    image.windows(16).find_map(|window| {
        let (signature, rest) = window.split_at(8);
        DESCRIPTOR_SIGNATURES
            .iter()
            .any(|s| s == signature)
            .then(|| u64::from_le_bytes(rest.try_into().expect("8 bytes")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_crc_after_either_signature() {
        for signature in DESCRIPTOR_SIGNATURES {
            let mut image = vec![0xFF; 100];
            image.extend_from_slice(&signature);
            image.extend_from_slice(&0x1122_3344_5566_7788u64.to_le_bytes());
            image.extend_from_slice(&[0; 40]);
            assert_eq!(app_descriptor_crc(&image), Some(0x1122_3344_5566_7788));
        }
    }

    #[test]
    fn an_image_without_a_descriptor_has_no_crc() {
        assert_eq!(app_descriptor_crc(&[0xAB; 500]), None);
        assert_eq!(app_descriptor_crc(&[]), None);
        // A signature too close to the end has no room for the CRC.
        let mut truncated = vec![0; 10];
        truncated.extend_from_slice(&DESCRIPTOR_SIGNATURES[0]);
        assert_eq!(app_descriptor_crc(&truncated), None);
    }
}
