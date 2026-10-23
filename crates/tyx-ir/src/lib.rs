use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

mod compat;

pub use compat::{Break, Compat, compat};

pub const MAX_OPTIONAL_FIELDS: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub types: Vec<Def>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    pub module: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
    #[serde(flatten)]
    pub shape: Shape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Newtype {
        ty: Ty,
    },
    Struct {
        fields: Fields,
    },
    Enum {
        variants: Vec<Variant>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tag: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "style", content = "fields", rename_all = "snake_case")]
pub enum Fields {
    Named(Vec<Field>),
    Tuple(Vec<Field>),
}

impl Fields {
    #[must_use]
    pub fn as_slice(&self) -> &[Field] {
        match self {
            Self::Named(fields) | Self::Tuple(fields) => fields,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub ty: Ty,
    #[serde(default, skip_serializing_if = "is_false")]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    /// The pinned wire index; `None` follows the previous variant's index, or is 0 for the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(flatten)]
    pub payload: Payload,
}

/// The wire index of each variant, in declaration order.
pub fn variant_indexes(variants: &[Variant]) -> impl Iterator<Item = u32> + '_ {
    variants.iter().scan(0u32, |next, variant| {
        let index = variant.index.unwrap_or(*next);
        *next = index.wrapping_add(1);
        Some(index)
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "payload", rename_all = "snake_case")]
pub enum Payload {
    Unit,
    Newtype { ty: Ty },
    Fields { fields: Fields },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ty {
    Unit,
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Str,
    Bytes,
    Uuid,
    Timestamp,
    Option {
        of: Box<Ty>,
    },
    Seq {
        of: Box<Ty>,
    },
    Set {
        of: Box<Ty>,
    },
    Map {
        key: Box<Ty>,
        value: Box<Ty>,
    },
    Array {
        of: Box<Ty>,
        len: usize,
    },
    Tuple {
        of: Vec<Ty>,
    },
    Named {
        name: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<Ty>,
    },
    Param {
        index: usize,
    },
}

impl Ty {
    #[must_use]
    pub fn has_zero_value(&self) -> bool {
        !matches!(self, Self::Uuid | Self::Timestamp | Self::Array { .. } | Self::Tuple { .. } | Self::Named { .. } | Self::Param { .. })
    }

    #[must_use]
    pub fn is_unit(&self) -> bool {
        *self == Self::Unit
    }

    /// Replaces `Param { index }` with `env[index]`; parameters without an argument stay as they are.
    #[must_use]
    pub fn substitute(&self, env: &[Ty]) -> Ty {
        match self {
            Self::Param { index } => env.get(*index).cloned().unwrap_or_else(|| self.clone()),
            Self::Option { of } => Self::Option { of: Box::new(of.substitute(env)) },
            Self::Seq { of } => Self::Seq { of: Box::new(of.substitute(env)) },
            Self::Set { of } => Self::Set { of: Box::new(of.substitute(env)) },
            Self::Array { of, len } => Self::Array { of: Box::new(of.substitute(env)), len: *len },
            Self::Map { key, value } => Self::Map { key: Box::new(key.substitute(env)), value: Box::new(value.substitute(env)) },
            Self::Tuple { of } => Self::Tuple { of: of.iter().map(|ty| ty.substitute(env)).collect() },
            Self::Named { name, args } => Self::Named { name: name.clone(), args: args.iter().map(|ty| ty.substitute(env)).collect() },
            _ => self.clone(),
        }
    }
}

/// Splits a Rust identifier into lowercase words: `HttpServer` and `http_server` both give `["http", "server"]`.
#[must_use]
pub fn words(ident: &str) -> Vec<String> {
    let chars: Vec<char> = ident.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();
    for (index, &ch) in chars.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| chars[i]);
        let next = chars.get(index + 1).copied();
        let starts_word = ch.is_uppercase()
            && previous
                .is_some_and(|p| p.is_lowercase() || p.is_ascii_digit() || (p.is_uppercase() && next.is_some_and(char::is_lowercase)));
        if ch == '_' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        if starts_word && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.extend(ch.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// Rust-like notation, the inverse of [`Ty::from_str`]: `Vec<Option<str>>`, `Map<str, u32>`, `[u8; 4]`, `Page<Book>`.
impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unit => f.write_str("()"),
            Self::Bool => f.write_str("bool"),
            Self::U8 => f.write_str("u8"),
            Self::U16 => f.write_str("u16"),
            Self::U32 => f.write_str("u32"),
            Self::U64 => f.write_str("u64"),
            Self::I8 => f.write_str("i8"),
            Self::I16 => f.write_str("i16"),
            Self::I32 => f.write_str("i32"),
            Self::I64 => f.write_str("i64"),
            Self::F32 => f.write_str("f32"),
            Self::F64 => f.write_str("f64"),
            Self::Str => f.write_str("str"),
            Self::Bytes => f.write_str("Vec<u8>"),
            Self::Uuid => f.write_str("Uuid"),
            Self::Timestamp => f.write_str("OffsetDateTime"),
            Self::Option { of } => write!(f, "Option<{of}>"),
            Self::Seq { of } => write!(f, "Vec<{of}>"),
            Self::Set { of } => write!(f, "Set<{of}>"),
            Self::Map { key, value } => write!(f, "Map<{key}, {value}>"),
            Self::Array { of, len } => write!(f, "[{of}; {len}]"),
            Self::Tuple { of } => {
                f.write_str("(")?;
                for (index, ty) in of.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                f.write_str(if of.len() == 1 { ",)" } else { ")" })
            }
            Self::Named { name, args } => {
                f.write_str(name)?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    for (index, ty) in args.iter().enumerate() {
                        if index > 0 {
                            f.write_str(", ")?;
                        }
                        write!(f, "{ty}")?;
                    }
                    f.write_str(">")?;
                }
                Ok(())
            }
            Self::Param { index } => write!(f, "${index}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTyError(String);

impl fmt::Display for ParseTyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseTyError {}

/// Also accepts the Rust spellings `String`, `Box<T>`, `Arc<T>`, `Rc<T>`, `HashSet`/`BTreeSet` and `HashMap`/`BTreeMap`.
impl std::str::FromStr for Ty {
    type Err = ParseTyError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parser = TyParser { text, pos: 0 };
        let ty = parser.ty()?;
        parser.skip_space();
        if parser.pos == text.len() { Ok(ty) } else { Err(parser.error("unexpected trailing input")) }
    }
}

struct TyParser<'a> {
    text: &'a str,
    pos: usize,
}

impl TyParser<'_> {
    fn error(&self, message: &str) -> ParseTyError {
        ParseTyError(format!("{message} at {} in `{}`", self.pos, self.text))
    }

    fn skip_space(&mut self) {
        self.pos += self.text[self.pos..].len() - self.text[self.pos..].trim_start().len();
    }

    fn eat(&mut self, token: char) -> bool {
        self.skip_space();
        if self.text[self.pos..].starts_with(token) {
            self.pos += token.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: char) -> Result<(), ParseTyError> {
        if self.eat(token) { Ok(()) } else { Err(self.error(&format!("expected `{token}`"))) }
    }

    fn word(&mut self) -> Result<&str, ParseTyError> {
        self.skip_space();
        let rest = &self.text[self.pos..];
        let len = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).unwrap_or(rest.len());
        if len == 0 {
            return Err(self.error("expected a type"));
        }
        self.pos += len;
        Ok(rest[..len].rsplit("::").next().unwrap_or_default())
    }

    fn list(&mut self, close: char) -> Result<Vec<Ty>, ParseTyError> {
        let mut items = Vec::new();
        while !self.eat(close) {
            items.push(self.ty()?);
            if !self.eat(',') {
                self.expect(close)?;
                break;
            }
        }
        Ok(items)
    }

    fn args(&mut self) -> Result<Vec<Ty>, ParseTyError> {
        if self.eat('<') { self.list('>') } else { Ok(Vec::new()) }
    }

    fn ty(&mut self) -> Result<Ty, ParseTyError> {
        if self.eat('(') {
            let of = self.list(')')?;
            return Ok(if of.is_empty() { Ty::Unit } else { Ty::Tuple { of } });
        }
        if self.eat('[') {
            let of = Box::new(self.ty()?);
            if self.eat(']') {
                return Ok(if *of == Ty::U8 { Ty::Bytes } else { Ty::Seq { of } });
            }
            self.expect(';')?;
            let len = self.word()?.parse().map_err(|_| self.error("expected an array length"))?;
            self.expect(']')?;
            return Ok(Ty::Array { of, len });
        }
        if self.eat('$') {
            let index = self.word()?.parse().map_err(|_| self.error("expected a parameter index"))?;
            return Ok(Ty::Param { index });
        }
        let name = self.word()?.to_owned();
        let mut args = self.args()?;
        let one = |args: &mut Vec<Ty>, parser: &Self| match args.len() {
            1 => Ok(Box::new(args.remove(0))),
            _ => Err(parser.error(&format!("`{name}` takes one type argument"))),
        };
        Ok(match name.as_str() {
            "bool" => Ty::Bool,
            "u8" => Ty::U8,
            "u16" => Ty::U16,
            "u32" => Ty::U32,
            "u64" | "usize" => Ty::U64,
            "i8" => Ty::I8,
            "i16" => Ty::I16,
            "i32" => Ty::I32,
            "i64" | "isize" => Ty::I64,
            "f32" => Ty::F32,
            "f64" => Ty::F64,
            "str" | "String" => Ty::Str,
            "Uuid" => Ty::Uuid,
            "OffsetDateTime" => Ty::Timestamp,
            "Box" | "Arc" | "Rc" => *one(&mut args, self)?,
            "Option" => Ty::Option { of: one(&mut args, self)? },
            "Vec" => match one(&mut args, self)? {
                of if *of == Ty::U8 => Ty::Bytes,
                of => Ty::Seq { of },
            },
            "Set" | "HashSet" | "BTreeSet" => Ty::Set { of: one(&mut args, self)? },
            "Map" | "HashMap" | "BTreeMap" => match <[Ty; 2]>::try_from(args) {
                Ok([key, value]) => Ty::Map { key: Box::new(key), value: Box::new(value) },
                Err(_) => return Err(self.error("`Map` takes two type arguments")),
            },
            _ => Ty::Named { name, args },
        })
    }
}

#[expect(clippy::trivially_copy_pass_by_ref, reason = "serde skip_serializing_if passes a reference")]
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Conflict { name: String, first: String, second: String },
    Unknown { name: String, referenced_by: String },
    DefaultWithoutZero { ty: String, field: String },
    TooManyOptional { ty: String },
    DuplicateVariantIndex { ty: String, index: u32 },
    InvalidOther { ty: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { name, first, second } => {
                write!(f, "type `{name}` is defined differently in `{first}` and `{second}`; rename one with #[tyx(name = \"...\")]")
            }
            Self::Unknown { name, referenced_by } => write!(f, "`{referenced_by}` references unknown type `{name}`"),
            Self::DefaultWithoutZero { ty, field } => {
                write!(f, "`{ty}.{field}` is marked default, but its type has no zero value")
            }
            Self::TooManyOptional { ty } => write!(f, "`{ty}` has more than {MAX_OPTIONAL_FIELDS} optional fields"),
            Self::DuplicateVariantIndex { ty, index } => write!(f, "`{ty}` has two variants with index {index}"),
            Self::InvalidOther { ty } => write!(f, "the `other` variant of `{ty}` must be a unit or hold a u8, u16 or u32"),
        }
    }
}

impl std::error::Error for Error {}

impl Schema {
    /// Combines schemas exported by several binaries; a name may repeat only with an identical definition.
    pub fn merge(schemas: impl IntoIterator<Item = Self>) -> Result<Self, Error> {
        let mut types = BTreeMap::<String, Def>::new();
        for def in schemas.into_iter().flat_map(|schema| schema.types) {
            match types.get(&def.name) {
                Some(known) if *known != def => {
                    return Err(Error::Conflict { name: def.name, first: known.module.clone(), second: def.module });
                }
                Some(_) => {}
                None => {
                    types.insert(def.name.clone(), def);
                }
            }
        }
        let schema = Self { types: types.into_values().collect() };
        schema.validate()?;
        Ok(schema)
    }

    #[must_use]
    pub fn def(&self, name: &str) -> Option<&Def> {
        self.types.iter().find(|def| def.name == name)
    }

    pub fn validate(&self) -> Result<(), Error> {
        for def in &self.types {
            let mut check = |ty: &Ty| self.check_refs(ty, &def.name);
            match &def.shape {
                Shape::Newtype { ty } => check(ty)?,
                Shape::Struct { fields } => check_fields(def, fields, &mut check)?,
                Shape::Enum { variants, other, .. } => {
                    let mut indexes: Vec<u32> = variant_indexes(variants).collect();
                    indexes.sort_unstable();
                    if let Some(pair) = indexes.windows(2).find(|pair| pair[0] == pair[1]) {
                        return Err(Error::DuplicateVariantIndex { ty: def.name.clone(), index: pair[0] });
                    }
                    if let Some(other) = other.and_then(|other| variants.get(other))
                        && !matches!(&other.payload, Payload::Unit | Payload::Newtype { ty: Ty::U8 | Ty::U16 | Ty::U32 })
                    {
                        return Err(Error::InvalidOther { ty: def.name.clone() });
                    }
                    for variant in variants {
                        match &variant.payload {
                            Payload::Unit => {}
                            Payload::Newtype { ty } => check(ty)?,
                            Payload::Fields { fields } => check_fields(def, fields, &mut check)?,
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Fails on the first named type in `ty` that this schema does not define; `owner` names the referrer in the error.
    pub fn check_refs(&self, ty: &Ty, owner: &str) -> Result<(), Error> {
        match ty {
            Ty::Option { of } | Ty::Seq { of } | Ty::Set { of } | Ty::Array { of, .. } => self.check_refs(of, owner),
            Ty::Map { key, value } => {
                self.check_refs(key, owner)?;
                self.check_refs(value, owner)
            }
            Ty::Tuple { of } => of.iter().try_for_each(|ty| self.check_refs(ty, owner)),
            Ty::Named { name, args } => {
                if self.def(name).is_none() {
                    return Err(Error::Unknown { name: name.clone(), referenced_by: owner.to_owned() });
                }
                args.iter().try_for_each(|ty| self.check_refs(ty, owner))
            }
            _ => Ok(()),
        }
    }

    /// The fields of a struct with named fields, type arguments substituted.
    #[must_use]
    pub fn named_fields(&self, ty: &Ty) -> Option<Vec<Field>> {
        let Ty::Named { name, args } = ty else {
            return None;
        };
        match &self.def(name)?.shape {
            Shape::Struct { fields: Fields::Named(fields) } => {
                Some(fields.iter().map(|field| Field { ty: field.ty.substitute(args), ..field.clone() }).collect())
            }
            _ => None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }
}

fn check_fields(def: &Def, fields: &Fields, check: &mut impl FnMut(&Ty) -> Result<(), Error>) -> Result<(), Error> {
    if fields.as_slice().iter().filter(|field| field.optional).count() > MAX_OPTIONAL_FIELDS {
        return Err(Error::TooManyOptional { ty: def.name.clone() });
    }
    for field in fields.as_slice() {
        if field.default && !field.optional && !field.ty.has_zero_value() {
            return Err(Error::DefaultWithoutZero { ty: def.name.clone(), field: field.name.clone() });
        }
        check(&field.ty)?;
    }
    Ok(())
}
