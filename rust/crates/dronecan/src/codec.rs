//! The DroneCAN (UAVCAN v0) wire encoding: a bit stream in which every field is written as
//! little-endian bytes, the most significant partial byte last, and each byte most significant
//! bit first (the layout `libcanard` produces). Encoding and decoding are driven by the parsed
//! DSDL, so every type in the catalog works without generated code.
use serde::{ser::SerializeMap, Serialize, Serializer};
use thiserror::Error;

use crate::dsdl::{Body, Cast, Catalog, Kind, Ty};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("`{0}` is not a field of this type")]
    UnknownField(String),
    #[error("field `{field}`: {message}")]
    Invalid { field: String, message: String },
    #[error("expected {0}")]
    Shape(&'static str),
    #[error("no such type")]
    UnknownType,
}

/// Which part of a type: a message, or the request or response of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Message,
    Request,
    Response,
}

/// A decoded (or to-be-encoded) value. `Struct` keeps declaration order and leaves out `void`
/// padding; a union is the chosen variant's name and its value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Uint(u64),
    Int(i64),
    Float(f64),
    Array(Vec<Value>),
    Struct(Vec<(String, Value)>),
    Union(String, Box<Value>),
}

impl Value {
    pub fn strukt<const N: usize>(fields: [(&str, Value); N]) -> Value {
        Value::Struct(fields.into_iter().map(|(n, v)| (n.to_owned(), v)).collect())
    }

    /// A byte string, as the `uint8[<=N]` text fields use.
    pub fn text(text: &str) -> Value {
        Value::Array(text.bytes().map(|b| Value::Uint(u64::from(b))).collect())
    }

    pub fn union(variant: &str, value: Value) -> Value {
        Value::Union(variant.to_owned(), Box::new(value))
    }

    /// A struct field, or a dotted path through nested structs (`status.health`).
    pub fn get(&self, path: &str) -> Option<&Value> {
        let mut current = self;
        for part in path.split('.') {
            current = match current {
                Value::Struct(fields) => &fields.iter().find(|(n, _)| n == part)?.1,
                Value::Union(name, inner) if name == part => inner,
                _ => return None,
            };
        }
        Some(current)
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Uint(v) => Some(*v),
            Value::Int(v) => u64::try_from(*v).ok(),
            Value::Bool(v) => Some(u64::from(*v)),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Uint(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(v) => Some(*v),
            Value::Uint(v) => Some(*v as f64),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            Value::Uint(v) => Some(*v != 0),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// An array of bytes as text (lossy).
    pub fn as_text(&self) -> Option<String> {
        let bytes: Option<Vec<u8>> = self
            .as_array()?
            .iter()
            .map(|v| v.as_u64().map(|b| b as u8))
            .collect();
        bytes.map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// The variant of a union.
    pub fn variant(&self) -> Option<(&str, &Value)> {
        match self {
            Value::Union(name, inner) => Some((name, inner)),
            _ => None,
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Bool(v) => serializer.serialize_bool(*v),
            Value::Uint(v) => serializer.serialize_u64(*v),
            Value::Int(v) => serializer.serialize_i64(*v),
            Value::Float(v) => serializer.serialize_f64(*v),
            Value::Array(items) => serializer.collect_seq(items),
            Value::Struct(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    map.serialize_entry(name, value)?;
                }
                map.end()
            }
            Value::Union(name, inner) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(name, inner)?;
                map.end()
            }
        }
    }
}

// ---- bit stream ----

#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    /// Up to 8 bits, most significant first.
    fn put_msb_first(&mut self, value: u8, count: usize) {
        for k in (0..count).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> k) & 1 == 1 {
                *self.bytes.last_mut().expect("a byte was pushed") |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }

    fn put(&mut self, value: u64, count: usize) {
        let full = count / 8;
        for i in 0..full {
            self.put_msb_first((value >> (8 * i)) as u8, 8);
        }
        let rest = count % 8;
        if rest > 0 {
            self.put_msb_first(((value >> (8 * full)) & ((1u64 << rest) - 1)) as u8, rest);
        }
    }

    fn skip(&mut self, count: usize) {
        for _ in 0..count {
            self.put_msb_first(0, 1);
        }
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    at: usize,
}

impl BitReader<'_> {
    fn total(&self) -> usize {
        self.data.len() * 8
    }

    fn remaining(&self) -> usize {
        self.total().saturating_sub(self.at)
    }

    fn bit(&mut self) -> u8 {
        let bit = self
            .data
            .get(self.at / 8)
            .map_or(0, |byte| (byte >> (7 - self.at % 8)) & 1);
        self.at += 1;
        bit
    }

    fn msb_first(&mut self, count: usize) -> u64 {
        (0..count).fold(0, |acc, _| (acc << 1) | u64::from(self.bit()))
    }

    /// Bits past the end of the payload read as zero (the implicit zero extension rule).
    fn get(&mut self, count: usize) -> u64 {
        let full = count / 8;
        let mut value = 0u64;
        for i in 0..full {
            value |= self.msb_first(8) << (8 * i);
        }
        let rest = count % 8;
        if rest > 0 {
            value |= self.msb_first(rest) << (8 * full);
        }
        value
    }
}

fn ceil_log2(n: usize) -> usize {
    if n <= 1 {
        0
    } else {
        (usize::BITS - (n - 1).leading_zeros()) as usize
    }
}

// ---- half precision ----

fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xFF) as i32;
    let mantissa = bits & 0x007F_FFFF;
    if exponent == 0xFF {
        return sign | 0x7C00 | if mantissa != 0 { 0x0200 } else { 0 };
    }
    let unbiased = exponent - 127 + 15;
    if unbiased >= 0x1F {
        return sign | 0x7BFF; // saturate to the largest finite half
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign;
        }
        let m = mantissa | 0x0080_0000;
        let shift = (14 - unbiased) as u32;
        let half = (m >> shift) as u16;
        let round = ((m >> (shift - 1)) & 1) as u16;
        return sign | (half + round);
    }
    let half = sign | ((unbiased as u16) << 10) | (mantissa >> 13) as u16;
    // Round to nearest.
    if (mantissa >> 12) & 1 == 1 && half & 0x7FFF != 0x7BFF {
        half + 1
    } else {
        half
    }
}

fn f16_to_f32(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exponent = u32::from((half >> 10) & 0x1F);
    let mantissa = u32::from(half & 0x03FF);
    let bits = match (exponent, mantissa) {
        (0, 0) => sign,
        (0, m) => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = m;
            while m & 0x0400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x03FF) << 13)
        }
        (0x1F, 0) => sign | 0x7F80_0000,
        (0x1F, m) => sign | 0x7F80_0000 | (m << 13),
        (e, m) => sign | ((e + 127 - 15) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

// ---- catalog driven encode / decode ----

impl Catalog {
    fn body_of(&self, type_index: usize, side: Side) -> Result<&Body, CodecError> {
        let def = self.types.get(type_index).ok_or(CodecError::UnknownType)?;
        match (&def.kind, side) {
            (Kind::Message(body), Side::Message) => Ok(body),
            (Kind::Service { request, .. }, Side::Request) => Ok(request),
            (Kind::Service { response, .. }, Side::Response) => Ok(response),
            _ => Err(CodecError::UnknownType),
        }
    }

    /// Fewest bits a value of the type occupies (an empty dynamic array counts as its prefix).
    fn min_bits(&self, ty: &Ty) -> usize {
        match ty {
            Ty::Bool(_) => 1,
            Ty::Uint { bits, .. }
            | Ty::Int { bits, .. }
            | Ty::Float { bits, .. }
            | Ty::Void(bits) => usize::from(*bits),
            Ty::Compound(index) => match &self.types[*index].kind {
                Kind::Message(body) => self.body_min_bits(body),
                Kind::Service { .. } => 0,
            },
            Ty::Array { elem, max, dynamic } => {
                if *dynamic {
                    ceil_log2(max + 1)
                } else {
                    self.min_bits(elem) * max
                }
            }
        }
    }

    fn body_min_bits(&self, body: &Body) -> usize {
        let fields: Vec<_> = body.fields().collect();
        if body.union {
            ceil_log2(fields.len())
                + fields
                    .iter()
                    .map(|f| self.min_bits(&f.ty))
                    .min()
                    .unwrap_or(0)
        } else {
            fields.iter().map(|f| self.min_bits(&f.ty)).sum()
        }
    }

    /// Encodes one side of a type. `tao` is the tail array optimisation: on in classic CAN, off
    /// in CAN FD.
    pub fn encode(
        &self,
        type_index: usize,
        side: Side,
        value: &Value,
        tao: bool,
    ) -> Result<Vec<u8>, CodecError> {
        let body = self.body_of(type_index, side)?;
        let mut writer = BitWriter::default();
        self.encode_body(&mut writer, body, value, tao)?;
        Ok(writer.bytes)
    }

    fn encode_body(
        &self,
        w: &mut BitWriter,
        body: &Body,
        value: &Value,
        tao: bool,
    ) -> Result<(), CodecError> {
        let fields: Vec<_> = body.fields().collect();
        if let Value::Struct(given) = value {
            for (name, _) in given {
                if !fields.iter().any(|f| &f.name == name) {
                    return Err(CodecError::UnknownField(name.clone()));
                }
            }
        }
        if body.union {
            // A union left out of its parent (an empty struct) takes its first variant.
            let empty = Value::Struct(Vec::new());
            let (variant, inner): (&str, &Value) = match value {
                Value::Union(variant, inner) => (variant, inner),
                Value::Struct(given) if given.is_empty() => {
                    (fields.first().map_or("", |f| f.name.as_str()), &empty)
                }
                _ => return Err(CodecError::Shape("a union value")),
            };
            let index = fields
                .iter()
                .position(|f| f.name == variant)
                .ok_or_else(|| CodecError::UnknownField(variant.to_owned()))?;
            w.put(index as u64, ceil_log2(fields.len()));
            return self.encode_field(
                w,
                &fields[index].name,
                &fields[index].ty,
                Some(inner),
                index + 1 == fields.len(),
                tao,
            );
        }
        let given: &[(String, Value)] = match value {
            Value::Struct(given) => given,
            _ => return Err(CodecError::Shape("a struct value")),
        };
        for (index, field) in fields.iter().enumerate() {
            let value = given.iter().find(|(n, _)| n == &field.name).map(|(_, v)| v);
            self.encode_field(
                w,
                &field.name,
                &field.ty,
                value,
                index + 1 == fields.len(),
                tao,
            )?;
        }
        Ok(())
    }

    fn encode_field(
        &self,
        w: &mut BitWriter,
        name: &str,
        ty: &Ty,
        value: Option<&Value>,
        is_last: bool,
        tao: bool,
    ) -> Result<(), CodecError> {
        let invalid = |message: &str| CodecError::Invalid {
            field: name.to_owned(),
            message: message.to_owned(),
        };
        match ty {
            Ty::Void(bits) => w.skip(usize::from(*bits)),
            Ty::Bool(_) => {
                let v = match value {
                    Some(v) => v.as_bool().ok_or_else(|| invalid("expected a boolean"))?,
                    None => false,
                };
                w.put(u64::from(v), 1);
            }
            Ty::Uint { bits, cast } => {
                let v = match value {
                    Some(v) => v
                        .as_u64()
                        .ok_or_else(|| invalid("expected an unsigned integer"))?,
                    None => 0,
                };
                let max = if *bits >= 64 {
                    u64::MAX
                } else {
                    (1u64 << bits) - 1
                };
                let v = match cast {
                    Cast::Saturated => v.min(max),
                    Cast::Truncated => v & max,
                };
                w.put(v, usize::from(*bits));
            }
            Ty::Int { bits, cast } => {
                let v = match value {
                    Some(v) => v.as_i64().ok_or_else(|| invalid("expected an integer"))?,
                    None => 0,
                };
                let bits_n = u32::from(*bits);
                let v = match cast {
                    Cast::Saturated if bits_n < 64 => {
                        let max = (1i64 << (bits_n - 1)) - 1;
                        v.clamp(-max - 1, max)
                    }
                    _ => v,
                };
                let mask = if bits_n >= 64 {
                    u64::MAX
                } else {
                    (1u64 << bits_n) - 1
                };
                w.put((v as u64) & mask, usize::from(*bits));
            }
            Ty::Float { bits, .. } => {
                let v = match value {
                    Some(v) => v.as_f64().ok_or_else(|| invalid("expected a number"))?,
                    None => 0.0,
                };
                match bits {
                    16 => w.put(u64::from(f32_to_f16(v as f32)), 16),
                    32 => w.put(u64::from((v as f32).to_bits()), 32),
                    64 => w.put(v.to_bits(), 64),
                    _ => return Err(invalid("unsupported float width")),
                }
            }
            Ty::Compound(index) => {
                let Kind::Message(body) = &self.types[*index].kind else {
                    return Err(invalid("a service cannot be a field"));
                };
                let empty = Value::Struct(Vec::new());
                self.encode_body(w, body, value.unwrap_or(&empty), is_last && tao)?;
            }
            Ty::Array { elem, max, dynamic } => {
                let items: &[Value] = match value {
                    Some(v) => v.as_array().ok_or_else(|| invalid("expected an array"))?,
                    None => &[],
                };
                if items.len() > *max {
                    return Err(invalid(&format!("at most {max} elements")));
                }
                if *dynamic {
                    let omit_length = is_last && tao && self.min_bits(elem) >= 8;
                    if !omit_length {
                        w.put(items.len() as u64, ceil_log2(max + 1));
                    }
                    for item in items {
                        self.encode_field(w, name, elem, Some(item), false, false)?;
                    }
                } else {
                    for i in 0..*max {
                        self.encode_field(w, name, elem, items.get(i), false, false)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Decodes one side of a type. Bits past the end of `bytes` read as zero.
    pub fn decode(
        &self,
        type_index: usize,
        side: Side,
        bytes: &[u8],
        tao: bool,
    ) -> Result<Value, CodecError> {
        let body = self.body_of(type_index, side)?;
        let mut reader = BitReader { data: bytes, at: 0 };
        self.decode_body(&mut reader, body, tao)
    }

    fn decode_body(
        &self,
        r: &mut BitReader<'_>,
        body: &Body,
        tao: bool,
    ) -> Result<Value, CodecError> {
        let fields: Vec<_> = body.fields().collect();
        if body.union {
            let tag = r.get(ceil_log2(fields.len())) as usize;
            let field = fields
                .get(tag)
                .ok_or(CodecError::Shape("a union tag that names a variant"))?;
            let value = self.decode_field(r, &field.ty, tag + 1 == fields.len(), tao)?;
            return Ok(Value::Union(
                field.name.clone(),
                Box::new(value.unwrap_or(Value::Struct(Vec::new()))),
            ));
        }
        let mut out = Vec::new();
        for (index, field) in fields.iter().enumerate() {
            if let Some(value) = self.decode_field(r, &field.ty, index + 1 == fields.len(), tao)? {
                out.push((field.name.clone(), value));
            }
        }
        Ok(Value::Struct(out))
    }

    fn decode_field(
        &self,
        r: &mut BitReader<'_>,
        ty: &Ty,
        is_last: bool,
        tao: bool,
    ) -> Result<Option<Value>, CodecError> {
        Ok(Some(match ty {
            Ty::Void(bits) => {
                r.at += usize::from(*bits);
                return Ok(None);
            }
            Ty::Bool(_) => Value::Bool(r.get(1) != 0),
            Ty::Uint { bits, .. } => Value::Uint(r.get(usize::from(*bits))),
            Ty::Int { bits, .. } => {
                let raw = r.get(usize::from(*bits));
                let bits = u32::from(*bits);
                Value::Int(if bits < 64 && raw & (1 << (bits - 1)) != 0 {
                    (raw | (u64::MAX << bits)) as i64
                } else {
                    raw as i64
                })
            }
            Ty::Float { bits, .. } => Value::Float(match bits {
                16 => f64::from(f16_to_f32(r.get(16) as u16)),
                32 => f64::from(f32::from_bits(r.get(32) as u32)),
                64 => f64::from_bits(r.get(64)),
                _ => return Err(CodecError::Shape("a float of 16, 32 or 64 bits")),
            }),
            Ty::Compound(index) => {
                let Kind::Message(body) = &self.types[*index].kind else {
                    return Err(CodecError::UnknownType);
                };
                self.decode_body(r, body, is_last && tao)?
            }
            Ty::Array { elem, max, dynamic } => {
                let elem_bits = self.min_bits(elem);
                let count = if *dynamic {
                    if is_last && tao && elem_bits >= 8 {
                        // No length prefix: the array runs to the end of the payload.
                        let remaining = r.remaining();
                        match **elem {
                            Ty::Compound(_) => usize::MAX,
                            _ => remaining / elem_bits.max(1),
                        }
                    } else {
                        r.get(ceil_log2(max + 1)) as usize
                    }
                } else {
                    *max
                };
                let mut items = Vec::new();
                if count == usize::MAX {
                    while r.remaining() > 0 && items.len() < *max {
                        items.push(
                            self.decode_field(r, elem, false, false)?
                                .unwrap_or(Value::Struct(Vec::new())),
                        );
                    }
                } else {
                    if count > *max {
                        return Err(CodecError::Shape("an array no longer than its maximum"));
                    }
                    for _ in 0..count {
                        items.push(
                            self.decode_field(r, elem, false, false)?
                                .unwrap_or(Value::Struct(Vec::new())),
                        );
                    }
                }
                Value::Array(items)
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(name: &str) -> usize {
        Catalog::builtin()
            .index_of(name)
            .unwrap_or_else(|| panic!("no type {name}"))
    }

    fn node_status() -> Value {
        Value::strukt([
            ("uptime_sec", Value::Uint(1000)),
            ("health", Value::Uint(1)),
            ("mode", Value::Uint(2)),
            ("sub_mode", Value::Uint(0)),
            ("vendor_specific_status_code", Value::Uint(0x1234)),
        ])
    }

    #[test]
    fn node_status_has_the_known_wire_layout() {
        let c = Catalog::builtin();
        let bytes = c
            .encode(
                index("uavcan.protocol.NodeStatus"),
                Side::Message,
                &node_status(),
                true,
            )
            .unwrap();
        // uptime LE, then health(2) mode(3) sub_mode(3) packed from the top bit, then the code LE.
        assert_eq!(
            bytes,
            vec![0xE8, 0x03, 0x00, 0x00, 0b01_010_000, 0x34, 0x12]
        );
        assert_eq!(
            c.decode(
                index("uavcan.protocol.NodeStatus"),
                Side::Message,
                &bytes,
                true
            )
            .unwrap(),
            node_status()
        );
    }

    #[test]
    fn get_set_requests_by_index_and_by_name() {
        let c = Catalog::builtin();
        let ty = index("uavcan.protocol.param.GetSet");
        let by_index = Value::strukt([
            ("index", Value::Uint(1)),
            ("value", Value::union("empty", Value::Struct(vec![]))),
            ("name", Value::text("")),
        ]);
        // index 1 in 13 bits, tag 0 in 3 bits, then the (empty) name without a length prefix.
        assert_eq!(
            c.encode(ty, Side::Request, &by_index, true).unwrap(),
            vec![0x01, 0x00]
        );
        let by_name = Value::strukt([
            ("index", Value::Uint(0)),
            ("value", Value::union("empty", Value::Struct(vec![]))),
            ("name", Value::text("FOO")),
        ]);
        assert_eq!(
            c.encode(ty, Side::Request, &by_name, true).unwrap(),
            vec![0x00, 0x00, b'F', b'O', b'O']
        );
        // CAN FD keeps the length prefix: 7 bits for up to 92 characters.
        let fd = c.encode(ty, Side::Request, &by_name, false).unwrap();
        assert_eq!(
            fd.len(),
            6,
            "16 bits + 7 length bits + 3 characters = 47 bits"
        );
        assert_eq!(
            c.decode(ty, Side::Request, &fd, false)
                .unwrap()
                .get("name")
                .unwrap()
                .as_text()
                .unwrap(),
            "FOO"
        );
        assert_eq!(
            c.decode(ty, Side::Request, &[0x00, 0x00, b'F', b'O', b'O'], true)
                .unwrap()
                .get("name")
                .unwrap()
                .as_text()
                .unwrap(),
            "FOO"
        );
    }

    #[test]
    fn unions_use_their_tag_and_variant() {
        let c = Catalog::builtin();
        let ty = index("uavcan.protocol.param.GetSet");
        let set = Value::strukt([
            ("index", Value::Uint(0)),
            ("value", Value::union("integer_value", Value::Int(-2))),
            ("name", Value::text("A")),
        ]);
        let bytes = c.encode(ty, Side::Request, &set, true).unwrap();
        let back = c.decode(ty, Side::Request, &bytes, true).unwrap();
        assert_eq!(back.get("value.integer_value").unwrap().as_i64(), Some(-2));
        assert_eq!(back.get("name").unwrap().as_text().unwrap(), "A");
        assert_eq!(
            back.get("value").unwrap().variant().unwrap().0,
            "integer_value"
        );
    }

    #[test]
    fn a_union_left_out_takes_its_first_variant() {
        let c = Catalog::builtin();
        let ty = index("uavcan.protocol.param.GetSet");
        let response = Value::strukt([("name", Value::text("X"))]);
        let bytes = c.encode(ty, Side::Response, &response, true).unwrap();
        let back = c.decode(ty, Side::Response, &bytes, true).unwrap();
        assert_eq!(back.get("value").unwrap().variant().unwrap().0, "empty");
        assert_eq!(back.get("max_value").unwrap().variant().unwrap().0, "empty");
        assert_eq!(back.get("name").unwrap().as_text().unwrap(), "X");
    }

    #[test]
    fn get_node_info_response_round_trips() {
        let c = Catalog::builtin();
        let ty = index("uavcan.protocol.GetNodeInfo");
        let response = Value::strukt([
            ("status", node_status()),
            (
                "software_version",
                Value::strukt([
                    ("major", Value::Uint(4)),
                    ("minor", Value::Uint(5)),
                    ("optional_field_flags", Value::Uint(3)),
                    ("vcs_commit", Value::Uint(0xDEADBEEF)),
                    ("image_crc", Value::Uint(0x0123_4567_89AB_CDEF)),
                ]),
            ),
            (
                "hardware_version",
                Value::strukt([
                    ("major", Value::Uint(1)),
                    ("minor", Value::Uint(2)),
                    (
                        "unique_id",
                        Value::Array((0..16).map(Value::Uint).collect()),
                    ),
                    ("certificate_of_authenticity", Value::Array(vec![])),
                ]),
            ),
            ("name", Value::text("com.example.gps")),
        ]);
        for tao in [true, false] {
            let bytes = c.encode(ty, Side::Response, &response, tao).unwrap();
            let back = c.decode(ty, Side::Response, &bytes, tao).unwrap();
            assert_eq!(back, response, "tao = {tao}");
        }
    }

    #[test]
    fn integers_saturate_or_truncate_and_signed_values_round_trip() {
        let c = Catalog::builtin();
        // GlobalTimeSync: truncated uint56.
        let ty = index("uavcan.protocol.GlobalTimeSync");
        let big = Value::strukt([(
            "previous_transmission_timestamp_usec",
            Value::Uint(u64::MAX),
        )]);
        let bytes = c.encode(ty, Side::Message, &big, true).unwrap();
        assert_eq!(bytes, vec![0xFF; 7]);
        // gnss.Fix has signed 37 and 27 bit fields.
        let fix = index("uavcan.equipment.gnss.Fix2");
        let value = Value::strukt([
            ("latitude_deg_1e8", Value::Int(-2_054_756_450)),
            ("longitude_deg_1e8", Value::Int(-4_742_026_140)),
        ]);
        let bytes = c.encode(fix, Side::Message, &value, true).unwrap();
        let back = c.decode(fix, Side::Message, &bytes, true).unwrap();
        assert_eq!(
            back.get("latitude_deg_1e8").unwrap().as_i64(),
            Some(-2_054_756_450)
        );
        assert_eq!(
            back.get("longitude_deg_1e8").unwrap().as_i64(),
            Some(-4_742_026_140)
        );
    }

    #[test]
    fn half_precision_floats_convert() {
        for value in [0.0f32, 1.0, -2.5, 0.333_251_95, 65504.0, 6.1035156e-5] {
            assert_eq!(f16_to_f32(f32_to_f16(value)), value, "{value}");
        }
        assert_eq!(f32_to_f16(1.0), 0x3C00);
        assert_eq!(f32_to_f16(1.0e9), 0x7BFF, "saturates");
        assert!(f16_to_f32(0x7E00).is_nan());
        assert_eq!(f16_to_f32(0xC000), -2.0);
    }

    #[test]
    fn short_payloads_decode_as_zero_extended() {
        let c = Catalog::builtin();
        let status = c
            .decode(
                index("uavcan.protocol.NodeStatus"),
                Side::Message,
                &[0x01],
                true,
            )
            .unwrap();
        assert_eq!(status.get("uptime_sec").unwrap().as_u64(), Some(1));
        assert_eq!(
            status.get("vendor_specific_status_code").unwrap().as_u64(),
            Some(0)
        );
    }

    #[test]
    fn bad_input_is_an_error() {
        let c = Catalog::builtin();
        let ty = index("uavcan.protocol.NodeStatus");
        assert_eq!(
            c.encode(
                ty,
                Side::Message,
                &Value::strukt([("nope", Value::Uint(1))]),
                true
            ),
            Err(CodecError::UnknownField("nope".into()))
        );
        assert!(c.encode(ty, Side::Request, &node_status(), true).is_err());
        let get_set = index("uavcan.protocol.param.GetSet");
        let too_long = Value::strukt([("name", Value::text(&"x".repeat(93)))]);
        assert!(c.encode(get_set, Side::Request, &too_long, true).is_err());
    }

    #[test]
    fn every_message_type_round_trips_its_defaults() {
        let c = Catalog::builtin();
        for (i, def) in c.types.iter().enumerate() {
            let sides: &[Side] = if def.is_service() {
                &[Side::Request, Side::Response]
            } else {
                &[Side::Message]
            };
            for &side in sides {
                let body = c.body_of(i, side).unwrap();
                // A union needs a chosen variant; use the first.
                let value = if body.union {
                    let first = body.fields().next().expect("union has a variant");
                    Value::union(&first.name, Value::Struct(vec![]))
                } else {
                    Value::Struct(vec![])
                };
                // The first variant may be a primitive that rejects an empty struct; skip those.
                for tao in [true, false] {
                    if let Ok(bytes) = c.encode(i, side, &value, tao) {
                        let back = c
                            .decode(i, side, &bytes, tao)
                            .unwrap_or_else(|e| panic!("{}: {e}", def.full_name));
                        let again = c
                            .encode(i, side, &back, tao)
                            .unwrap_or_else(|e| panic!("{}: {e}", def.full_name));
                        assert_eq!(bytes, again, "{} {side:?} tao={tao}", def.full_name);
                    }
                }
            }
        }
    }
}
