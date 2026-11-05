//! Decoder for ArduPilot's packed parameter file (`@PARAM/param.pck`, fetched over MAVFTP):
//! a much faster way to read every parameter than PARAM_REQUEST_LIST.
use crate::{Param, ParamType};

const MAGIC: u16 = 0x671b;
const MAGIC_WITH_DEFAULTS: u16 = 0x671c;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PckError {
    #[error("param.pck is too short ({0} bytes)")]
    TooShort(usize),
    #[error("not a param.pck file (magic {0:#06x})")]
    BadMagic(u16),
    #[error("param.pck is truncated")]
    Truncated,
    #[error("param.pck has an unknown parameter type {0}")]
    BadType(u8),
    #[error("param.pck declares {declared} parameters but holds {found}")]
    CountMismatch { declared: u16, found: usize },
}

/// Parameters in file order; `index` is the position. Default values (magic 0x671c) are skipped.
pub fn unpack_param_pck(data: &[u8]) -> Result<Vec<Param>, PckError> {
    if data.len() < 6 {
        return Err(PckError::TooShort(data.len()));
    }
    let magic = u16::from_le_bytes([data[0], data[1]]);
    let declared = u16::from_le_bytes([data[2], data[3]]);
    let total = u16::from_le_bytes([data[4], data[5]]);
    if magic != MAGIC && magic != MAGIC_WITH_DEFAULTS {
        return Err(PckError::BadMagic(magic));
    }
    let with_defaults = magic == MAGIC_WITH_DEFAULTS;

    let mut pos = 6;
    let mut last_name = String::new();
    let mut params = Vec::new();
    loop {
        // Zero bytes are padding that keeps a value from crossing a read packet boundary.
        while pos < data.len() && data[pos] == 0 {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let head = *data.get(pos).ok_or(PckError::Truncated)?;
        let lens = *data.get(pos + 1).ok_or(PckError::Truncated)?;
        pos += 2;
        let (kind, flags) = (head & 0x0F, head >> 4);
        let (param_type, width) = match kind {
            1 => (ParamType::Int8, 1),
            2 => (ParamType::Int16, 2),
            3 => (ParamType::Int32, 4),
            4 => (ParamType::Real32, 4),
            other => return Err(PckError::BadType(other)),
        };
        let name_len = usize::from(lens >> 4) + 1;
        let common = usize::from(lens & 0x0F).min(last_name.len());
        let suffix = data.get(pos..pos + name_len).ok_or(PckError::Truncated)?;
        pos += name_len;
        let name = format!(
            "{}{}",
            &last_name[..common],
            suffix.iter().map(|&b| b as char).collect::<String>()
        );
        let raw = data.get(pos..pos + width).ok_or(PckError::Truncated)?;
        pos += width;
        if with_defaults && flags & 1 != 0 {
            // a distinct default value follows the value
            data.get(pos..pos + width).ok_or(PckError::Truncated)?;
            pos += width;
        }
        let value = match kind {
            1 => f64::from(raw[0] as i8),
            2 => f64::from(i16::from_le_bytes([raw[0], raw[1]])),
            3 => f64::from(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
            _ => f64::from(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])),
        };
        params.push(Param {
            name: name.clone(),
            value,
            param_type,
            index: params.len() as u16,
        });
        last_name = name;
    }
    if params.len() != usize::from(declared) || params.len() > usize::from(total) {
        return Err(PckError::CountMismatch {
            declared,
            found: params.len(),
        });
    }
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// header + entries built by hand following the documented layout
    fn file(magic: u16, declared: u16, total: u16, body: &[u8]) -> Vec<u8> {
        let mut v = magic.to_le_bytes().to_vec();
        v.extend(declared.to_le_bytes());
        v.extend(total.to_le_bytes());
        v.extend(body);
        v
    }

    #[test]
    fn decodes_types_common_prefix_and_padding() {
        let mut body = vec![];
        // "SERIAL1_BAUD" as int16 = 57: common 0, name_len-1 = 11 (high nibble)
        body.extend([2, 11 << 4]);
        body.extend(b"SERIAL1_BAUD");
        body.extend(57i16.to_le_bytes());
        body.extend([0, 0]); // padding
                             // INT32 sharing "SERIAL1_" (8 chars) with suffix "OPTIONS"
        body.extend([3, (6 << 4) | 8]);
        body.extend(b"OPTIONS");
        body.extend((-5i32).to_le_bytes());
        // REAL32
        body.extend([4, 3 << 4]); // common 0, name_len 4
        body.extend(b"GAIN");
        body.extend(1.5f32.to_le_bytes());
        // INT8
        body.extend([1, 0]);
        body.extend(b"X");
        body.push(0xFE);
        let params = unpack_param_pck(&file(MAGIC, 4, 4, &body)).unwrap();
        let got: Vec<_> = params.iter().map(|p| (p.name.as_str(), p.value)).collect();
        assert_eq!(
            got,
            [
                ("SERIAL1_BAUD", 57.0),
                ("SERIAL1_OPTIONS", -5.0),
                ("GAIN", 1.5),
                ("X", -2.0)
            ]
        );
        assert_eq!(params[2].param_type, ParamType::Real32);
        assert_eq!(params[3].index, 3);
    }

    #[test]
    fn skips_embedded_defaults() {
        let mut body = vec![];
        body.extend([4 | (1 << 4), 0]); // float with a separate default
        body.extend(b"A");
        body.extend(2.0f32.to_le_bytes());
        body.extend(9.0f32.to_le_bytes());
        body.extend([4, 0]); // float whose default equals the value
        body.extend(b"B");
        body.extend(3.0f32.to_le_bytes());
        let params = unpack_param_pck(&file(MAGIC_WITH_DEFAULTS, 2, 2, &body)).unwrap();
        assert_eq!((params[0].value, params[1].value), (2.0, 3.0));
    }

    #[test]
    fn rejects_bad_files() {
        assert_eq!(unpack_param_pck(&[1, 2]), Err(PckError::TooShort(2)));
        assert_eq!(
            unpack_param_pck(&file(0x1234, 0, 0, &[])),
            Err(PckError::BadMagic(0x1234))
        );
        assert_eq!(
            unpack_param_pck(&file(MAGIC, 1, 1, &[9, 0, b'A', 0])),
            Err(PckError::BadType(9))
        );
        assert_eq!(
            unpack_param_pck(&file(MAGIC, 1, 1, &[3, 0, b'A', 1])),
            Err(PckError::Truncated)
        );
        assert_eq!(
            unpack_param_pck(&file(MAGIC, 2, 2, &[1, 0, b'A', 1])),
            Err(PckError::CountMismatch {
                declared: 2,
                found: 1
            })
        );
    }
}
