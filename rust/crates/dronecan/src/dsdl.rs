//! DSDL v0 (the DroneCAN / UAVCAN v0 data structure description language): a parser for the
//! vendored `.uavcan` files and the 64-bit data type signature that seeds the multi-frame CRC.
use std::{collections::HashMap, sync::OnceLock};

use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/dsdl_files.rs"));

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DsdlError {
    #[error("{file}: {message}")]
    Parse { file: String, message: String },
    #[error("{file}: unknown type `{name}`")]
    UnknownType { file: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cast {
    Saturated,
    Truncated,
}

impl Cast {
    fn word(self) -> &'static str {
        match self {
            Self::Saturated => "saturated",
            Self::Truncated => "truncated",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Bool(Cast),
    Uint {
        bits: u8,
        cast: Cast,
    },
    Int {
        bits: u8,
        cast: Cast,
    },
    Float {
        bits: u8,
        cast: Cast,
    },
    Void(u8),
    /// Index into [`Catalog::types`].
    Compound(usize),
    Array {
        elem: Box<Ty>,
        max: usize,
        dynamic: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Empty for `voidN` padding.
    pub name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i128),
    Float(f64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    pub name: String,
    pub ty: Ty,
    pub value: ConstValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Field(Field),
    Const(Constant),
}

/// One side of a message or service: its fields and constants in declaration order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Body {
    pub union: bool,
    pub items: Vec<Item>,
}

impl Body {
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|item| match item {
            Item::Field(f) => Some(f),
            Item::Const(_) => None,
        })
    }

    pub fn constants(&self) -> impl Iterator<Item = &Constant> {
        self.items.iter().filter_map(|item| match item {
            Item::Const(c) => Some(c),
            Item::Field(_) => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Message(Body),
    Service { request: Body, response: Body },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    /// `uavcan.protocol.NodeStatus`.
    pub full_name: String,
    pub id: Option<u16>,
    pub kind: Kind,
    /// The 64-bit data type signature.
    pub signature: u64,
}

impl TypeDef {
    pub fn is_service(&self) -> bool {
        matches!(self.kind, Kind::Service { .. })
    }
}

#[derive(Debug, Default)]
pub struct Catalog {
    pub types: Vec<TypeDef>,
    by_name: HashMap<String, usize>,
    messages: HashMap<u16, usize>,
    services: HashMap<u16, usize>,
}

impl Catalog {
    /// The catalog of every vendored definition, parsed once.
    pub fn builtin() -> &'static Catalog {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| Catalog::parse(DSDL_FILES).expect("embedded DSDL parses"))
    }

    pub fn by_name(&self, full_name: &str) -> Option<&TypeDef> {
        self.by_name.get(full_name).map(|&i| &self.types[i])
    }

    pub fn index_of(&self, full_name: &str) -> Option<usize> {
        self.by_name.get(full_name).copied()
    }

    pub fn message(&self, id: u16) -> Option<&TypeDef> {
        self.messages.get(&id).map(|&i| &self.types[i])
    }

    pub fn service(&self, id: u16) -> Option<&TypeDef> {
        self.services.get(&id).map(|&i| &self.types[i])
    }

    /// Parses `(path relative to the dsdl root, text)` pairs.
    pub fn parse(files: &[(&str, &str)]) -> Result<Catalog, DsdlError> {
        let mut catalog = Catalog::default();
        // Pass 1: names and ids, so compound fields can refer to any type.
        for (path, _) in files {
            let (namespace, id, name) = split_path(path)?;
            let full_name = format!("{namespace}.{name}");
            catalog
                .by_name
                .insert(full_name.clone(), catalog.types.len());
            catalog.types.push(TypeDef {
                full_name,
                id,
                kind: Kind::Message(Body::default()),
                signature: 0,
            });
        }
        // Pass 2: bodies.
        let mut forced_signature: Vec<Option<u64>> = vec![None; files.len()];
        for (index, (path, text)) in files.iter().enumerate() {
            let (namespace, ..) = split_path(path)?;
            let (kind, forced) = catalog.parse_body(path, &namespace, text)?;
            catalog.types[index].kind = kind;
            catalog.types[index].signature = forced.unwrap_or(0);
            forced_signature[index] = forced;
        }
        for (index, def) in catalog.types.iter().enumerate() {
            if let Some(id) = def.id {
                if def.is_service() {
                    catalog.services.insert(id, index);
                } else {
                    catalog.messages.insert(id, index);
                }
            }
        }
        // A definition moved from another namespace keeps its original signature.
        let mut memo = forced_signature;
        for index in 0..catalog.types.len() {
            let signature = catalog.signature_of(index, &mut memo);
            catalog.types[index].signature = signature;
        }
        Ok(catalog)
    }

    fn parse_body(
        &self,
        file: &str,
        namespace: &str,
        text: &str,
    ) -> Result<(Kind, Option<u64>), DsdlError> {
        let fail = |message: String| DsdlError::Parse {
            file: file.to_owned(),
            message,
        };
        let override_signature = std::cell::Cell::new(None);
        let mut sections = vec![Body::default()];
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if line == "---" {
                sections.push(Body::default());
                continue;
            }
            let body = sections.last_mut().expect("at least one section");
            if let Some(value) = line.strip_prefix("OVERRIDE_SIGNATURE") {
                let value = value.trim();
                let hex = value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                    .unwrap_or(value);
                let parsed = u64::from_str_radix(hex, 16)
                    .map_err(|_| fail(format!("bad signature `{value}`")))?;
                override_signature.set(Some(parsed));
                continue;
            }
            if line == "@union" {
                body.union = true;
                continue;
            }
            body.items
                .push(self.parse_statement(file, namespace, line).map_err(fail)?);
        }
        let kind = match sections.len() {
            1 => Kind::Message(sections.remove(0)),
            2 => {
                let response = sections.remove(1);
                Kind::Service {
                    request: sections.remove(0),
                    response,
                }
            }
            _ => return Err(fail("more than one `---`".into())),
        };
        Ok((kind, override_signature.get()))
    }

    fn parse_statement(&self, file: &str, namespace: &str, line: &str) -> Result<Item, String> {
        // A constant has an `=` outside the array brackets (`float16[<=255] x` is a field).
        let mut depth = 0usize;
        let assignment = line.char_indices().find_map(|(at, c)| match c {
            '[' => {
                depth += 1;
                None
            }
            ']' => {
                depth = depth.saturating_sub(1);
                None
            }
            '=' if depth == 0 => Some(at),
            _ => None,
        });
        let (declaration, init) = match assignment {
            Some(at) => (line[..at].trim(), Some(line[at + 1..].trim())),
            None => (line, None),
        };
        let mut tokens = declaration.split_whitespace().peekable();
        let mut cast = Cast::Saturated;
        if let Some(&word) = tokens.peek() {
            if word == "saturated" || word == "truncated" {
                cast = if word == "saturated" {
                    Cast::Saturated
                } else {
                    Cast::Truncated
                };
                tokens.next();
            }
        }
        let type_text = tokens.next().ok_or("missing type")?;
        let name = tokens.next();
        if tokens.next().is_some() {
            return Err(format!("unexpected tokens in `{line}`"));
        }
        let ty = self.parse_type(file, namespace, type_text, cast)?;
        match (init, name) {
            (Some(init), Some(name)) => Ok(Item::Const(Constant {
                name: name.to_owned(),
                value: parse_constant(init)?,
                ty,
            })),
            (None, Some(name)) => Ok(Item::Field(Field {
                name: name.to_owned(),
                ty,
            })),
            (None, None) if matches!(ty, Ty::Void(_)) => Ok(Item::Field(Field {
                name: String::new(),
                ty,
            })),
            _ => Err(format!("cannot parse `{line}`")),
        }
    }

    fn parse_type(
        &self,
        file: &str,
        namespace: &str,
        text: &str,
        cast: Cast,
    ) -> Result<Ty, String> {
        let (base, array) = match text.find('[') {
            Some(at) => (&text[..at], Some(&text[at..])),
            None => (text, None),
        };
        let element = if let Some(bits) = base.strip_prefix("void").filter(|r| !r.is_empty()) {
            Ty::Void(
                bits.parse()
                    .map_err(|_| format!("bad void width in `{text}`"))?,
            )
        } else if base == "bool" {
            Ty::Bool(cast)
        } else if let Some(bits) = base.strip_prefix("uint").and_then(|b| b.parse().ok()) {
            Ty::Uint { bits, cast }
        } else if let Some(bits) = base.strip_prefix("int").and_then(|b| b.parse().ok()) {
            Ty::Int { bits, cast }
        } else if let Some(bits) = base.strip_prefix("float").and_then(|b| b.parse().ok()) {
            Ty::Float { bits, cast }
        } else {
            let local = format!("{namespace}.{base}");
            let index = self
                .by_name
                .get(&local)
                .or_else(|| self.by_name.get(base))
                .ok_or_else(|| {
                    DsdlError::UnknownType {
                        file: file.to_owned(),
                        name: base.to_owned(),
                    }
                    .to_string()
                })?;
            Ty::Compound(*index)
        };
        let Some(array) = array else {
            return Ok(element);
        };
        let inner = array
            .strip_prefix('[')
            .and_then(|a| a.strip_suffix(']'))
            .ok_or_else(|| format!("bad array in `{text}`"))?;
        let (max, dynamic) = if let Some(n) = inner.strip_prefix("<=") {
            (
                n.trim()
                    .parse::<usize>()
                    .map_err(|_| format!("bad array size in `{text}`"))?,
                true,
            )
        } else if let Some(n) = inner.strip_prefix('<') {
            let n = n
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("bad array size in `{text}`"))?;
            (
                n.checked_sub(1)
                    .ok_or_else(|| format!("bad array size in `{text}`"))?,
                true,
            )
        } else {
            (
                inner
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("bad array size in `{text}`"))?,
                false,
            )
        };
        Ok(Ty::Array {
            elem: Box::new(element),
            max,
            dynamic,
        })
    }

    /// The definition text the signature is computed over (pyuavcan's
    /// `get_dsdl_signature_source_definition`).
    fn signature_source(&self, index: usize) -> String {
        let def = &self.types[index];
        let adjoin = |text: &mut String, body: &Body| {
            // Constants are not part of the signature, so they can be added compatibly.
            let lines: Vec<String> = body
                .items
                .iter()
                .filter(|i| matches!(i, Item::Field(_)))
                .map(|i| self.normalized_item(i))
                .collect();
            text.push_str(&lines.join("\n"));
            text.push('\n');
        };
        let mut text = format!("{}\n", def.full_name);
        match &def.kind {
            Kind::Message(body) => {
                if body.union {
                    text.push_str("\n@union\n");
                }
                adjoin(&mut text, body);
            }
            Kind::Service { request, response } => {
                if request.union {
                    text.push_str("\n@union\n");
                }
                adjoin(&mut text, request);
                text.push_str("\n---\n");
                if response.union {
                    text.push_str("\n@union\n");
                }
                adjoin(&mut text, response);
            }
        }
        text.trim().replace("\n\n\n", "\n").replace("\n\n", "\n")
    }

    fn normalized_type(&self, ty: &Ty) -> String {
        match ty {
            Ty::Bool(cast) => format!("{} bool", cast.word()),
            Ty::Uint { bits, cast } => format!("{} uint{bits}", cast.word()),
            Ty::Int { bits, cast } => format!("{} int{bits}", cast.word()),
            Ty::Float { bits, cast } => format!("{} float{bits}", cast.word()),
            Ty::Void(bits) => format!("void{bits}"),
            Ty::Compound(index) => self.types[*index].full_name.clone(),
            Ty::Array { elem, max, dynamic } => {
                let inner = self.normalized_type(elem);
                if *dynamic {
                    format!("{inner}[<={max}]")
                } else {
                    format!("{inner}[{max}]")
                }
            }
        }
    }

    fn normalized_item(&self, item: &Item) -> String {
        match item {
            Item::Field(f) if f.name.is_empty() => self.normalized_type(&f.ty),
            Item::Field(f) => format!("{} {}", self.normalized_type(&f.ty), f.name),
            Item::Const(c) => {
                let value = match &c.value {
                    ConstValue::Int(v) => v.to_string(),
                    ConstValue::Float(v) => format!("{v:?}"),
                    ConstValue::Bool(v) => if *v { "True" } else { "False" }.to_owned(),
                };
                format!("{} {} = {}", self.normalized_type(&c.ty), c.name, value)
            }
        }
    }

    fn signature_of(&self, index: usize, memo: &mut Vec<Option<u64>>) -> u64 {
        if let Some(done) = memo[index] {
            return done;
        }
        let mut crc = Crc64::new(None);
        crc.add(self.signature_source(index).as_bytes());
        let mut sig = Crc64::new(Some(crc.value()));
        let def = &self.types[index];
        let fields: Vec<&Field> = match &def.kind {
            Kind::Message(body) => body.fields().collect(),
            Kind::Service { request, response } => {
                request.fields().chain(response.fields()).collect()
            }
        };
        for field in fields {
            let compound = match &field.ty {
                Ty::Compound(i) => Some(*i),
                Ty::Array { elem, .. } => match **elem {
                    Ty::Compound(i) => Some(i),
                    _ => None,
                },
                _ => None,
            };
            if let Some(i) = compound {
                let field_signature = self.signature_of(i, memo);
                let before = sig.value();
                sig.add(&field_signature.to_le_bytes());
                sig.add(&before.to_le_bytes());
            }
        }
        let value = sig.value();
        memo[index] = Some(value);
        value
    }
}

fn split_path(path: &str) -> Result<(String, Option<u16>, String), DsdlError> {
    let fail = |message: &str| DsdlError::Parse {
        file: path.to_owned(),
        message: message.to_owned(),
    };
    let stem = path
        .strip_suffix(".uavcan")
        .ok_or_else(|| fail("not a .uavcan file"))?;
    let (dir, file) = stem.rsplit_once('/').ok_or_else(|| fail("no namespace"))?;
    let namespace = dir.replace('/', ".");
    Ok(match file.split_once('.') {
        Some((id, name)) if id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() => (
            namespace,
            Some(id.parse().map_err(|_| fail("bad data type id"))?),
            name.to_owned(),
        ),
        _ => (namespace, None, file.to_owned()),
    })
}

/// Drops a `#` comment, unless the `#` is inside a quoted character constant.
fn strip_comment(line: &str) -> &str {
    let mut in_quote = false;
    for (at, c) in line.char_indices() {
        match c {
            '\'' => in_quote = !in_quote,
            '#' if !in_quote => return &line[..at],
            _ => {}
        }
    }
    line
}

fn parse_constant(text: &str) -> Result<ConstValue, String> {
    if let Some(inner) = text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        let value = match inner {
            "\\n" => 10,
            "\\r" => 13,
            "\\t" => 9,
            "\\\\" => 92,
            "\\'" => 39,
            other if other.chars().count() == 1 => other.chars().next().unwrap() as i128,
            other => return Err(format!("bad character constant `{other}`")),
        };
        return Ok(ConstValue::Int(value));
    }
    match text {
        "true" => return Ok(ConstValue::Bool(true)),
        "false" => return Ok(ConstValue::Bool(false)),
        _ => {}
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let int = if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        i128::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = digits.strip_prefix("0b") {
        i128::from_str_radix(bin, 2).ok()
    } else {
        digits.parse::<i128>().ok()
    };
    if let Some(value) = int {
        return Ok(ConstValue::Int(if negative { -value } else { value }));
    }
    text.parse::<f64>()
        .map(ConstValue::Float)
        .map_err(|_| format!("bad constant `{text}`"))
}

/// CRC-64/WE (polynomial 0x42F0E1EBA9EA3693, no reflection) as used for DSDL signatures.
struct Crc64(u64);

impl Crc64 {
    const POLY: u64 = 0x42F0_E1EB_A9EA_3693;

    /// `extend_from` continues a signature that was already finalized.
    fn new(extend_from: Option<u64>) -> Self {
        Self(match extend_from {
            Some(value) => value ^ u64::MAX,
            None => u64::MAX,
        })
    }

    fn add(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte) << 56;
            for _ in 0..8 {
                self.0 = if self.0 & (1 << 63) != 0 {
                    (self.0 << 1) ^ Self::POLY
                } else {
                    self.0 << 1
                };
            }
        }
    }

    fn value(&self) -> u64 {
        self.0 ^ u64::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golden() -> Vec<(String, u16, u64)> {
        include_str!("../tests/golden_signatures.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let mut parts = l.split_whitespace();
                let name = parts.next().unwrap().to_owned();
                let id = parts.next().unwrap().parse().unwrap();
                let sig = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
                (name, id, sig)
            })
            .collect()
    }

    #[test]
    fn crc64_matches_the_check_value() {
        // CRC-64/WE check value of "123456789".
        let mut crc = Crc64::new(None);
        crc.add(b"123456789");
        assert_eq!(crc.value(), 0x62EC_59E3_F1A4_F00A);
    }

    #[test]
    fn the_embedded_catalog_parses() {
        let catalog = Catalog::builtin();
        assert!(catalog.types.len() >= 140);
        let status = catalog.message(341).expect("NodeStatus");
        assert_eq!(status.full_name, "uavcan.protocol.NodeStatus");
        assert!(catalog.service(1).expect("GetNodeInfo").is_service());
        assert_eq!(
            catalog.message(5).unwrap().full_name,
            "uavcan.protocol.Panic"
        );
        assert_eq!(
            catalog.service(5).unwrap().full_name,
            "uavcan.protocol.RestartNode"
        );
    }

    #[test]
    fn every_signature_matches_the_legacy_table() {
        let catalog = Catalog::builtin();
        let mut checked = 0;
        let mut wrong = Vec::new();
        for (underscored, id, expected) in golden() {
            let base = underscored
                .trim_end_matches("_req")
                .trim_end_matches("_res");
            let def = catalog
                .types
                .iter()
                .find(|t| t.full_name.replace('.', "_") == base && t.id == Some(id))
                .unwrap_or_else(|| panic!("no definition for {underscored} ({id})"));
            checked += 1;
            if def.signature != expected {
                wrong.push(format!(
                    "{} id {id}: got {:016X}, want {expected:016X}",
                    def.full_name, def.signature
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {checked} signatures differ, e.g.\n{}",
            wrong.len(),
            wrong
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(checked, 155);
    }
}
