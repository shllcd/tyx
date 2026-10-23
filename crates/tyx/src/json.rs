//! The wire format read and written as JSON through the IR rather than through Rust types, for
//! tools that hold bytes and a type name but not the type itself.
//!
//! Field and variant names are the Rust ones; enums are externally tagged (`"Unit"`,
//! `{"Variant": payload}`; an `other` variant holding the index reads `{"Other": 7}`), uuids are
//! hyphenated strings, bytes are lowercase hex strings, timestamps are RFC 3339, maps with
//! string keys are objects and other maps are arrays of `[key, value]`. An optional field is
//! absent from the object when it is absent on the wire.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Number, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tyx_ir::{Def, Field, Fields, Payload, Schema, Shape, Ty, variant_indexes};

use crate::{Reader, Writer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() { f.write_str(&self.message) } else { write!(f, "{}: {}", self.path, self.message) }
    }
}

impl std::error::Error for Error {}

pub struct Codec<'s> {
    types: BTreeMap<&'s str, &'s Def>,
}

impl<'s> Codec<'s> {
    #[must_use]
    pub fn new(schema: &'s Schema) -> Self {
        Self { types: schema.types.iter().map(|def| (def.name.as_str(), def)).collect() }
    }

    pub fn encode(&self, ty: &Ty, value: &Value) -> Result<Vec<u8>, Error> {
        let mut cursor = Cursor { codec: self, path: Vec::new() };
        let mut w = Writer::new();
        cursor.write(&mut w, ty, &[], value)?;
        Ok(w.into_vec())
    }

    pub fn decode(&self, ty: &Ty, bytes: &[u8]) -> Result<Value, Error> {
        let mut cursor = Cursor { codec: self, path: Vec::new() };
        let mut r = Reader::new(bytes);
        let value = cursor.read(&mut r, ty, &[])?;
        if r.is_empty() { Ok(value) } else { Err(cursor.fail(crate::Error::TrailingBytes)) }
    }
}

struct Cursor<'c, 's> {
    codec: &'c Codec<'s>,
    path: Vec<String>,
}

fn optional_bits(fields: &[Field]) -> impl Iterator<Item = (Option<u32>, &Field)> {
    fields.iter().scan(0u32, |next, field| {
        let bit = field.optional.then(|| {
            *next += 1;
            *next - 1
        });
        Some((bit, field))
    })
}

fn zero_value(ty: &Ty) -> Value {
    match ty {
        Ty::Bool => Value::Bool(false),
        Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 => Value::from(0),
        Ty::F32 | Ty::F64 => Value::from(0.0),
        Ty::Str | Ty::Bytes => Value::String(String::new()),
        Ty::Seq { .. } | Ty::Set { .. } => Value::Array(Vec::new()),
        Ty::Map { key, .. } if **key == Ty::Str => Value::Object(Map::new()),
        Ty::Map { .. } => Value::Array(Vec::new()),
        _ => Value::Null,
    }
}

fn hex_uuid(bytes: [u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let (pairs, odd) = text.as_bytes().as_chunks::<2>();
    if !odd.is_empty() {
        return None;
    }
    pairs.iter().map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect()
}

fn parse_uuid(text: &str) -> Option<[u8; 16]> {
    let digits: Vec<u8> = text.bytes().filter(|&byte| byte != b'-').collect();
    if digits.len() != 32 || text.len() != 36 {
        return None;
    }
    let mut out = [0u8; 16];
    for (slot, pair) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

impl<'s> Cursor<'_, 's> {
    fn fail(&self, message: impl fmt::Display) -> Error {
        Error { path: self.path.concat(), message: message.to_string() }
    }

    fn def(&self, name: &str) -> Result<&'s Def, Error> {
        self.codec.types.get(name).copied().ok_or_else(|| self.fail(format_args!("unknown type `{name}`")))
    }

    fn scoped<T>(&mut self, segment: String, run: impl FnOnce(&mut Self) -> Result<T, Error>) -> Result<T, Error> {
        self.path.push(segment);
        let result = run(self);
        self.path.pop();
        result
    }

    fn int(&self, value: &Value, min: i128, max: i128) -> Result<i128, Error> {
        let number = value.as_i64().map(i128::from).or_else(|| value.as_u64().map(i128::from));
        match number {
            Some(number) if (min..=max).contains(&number) => Ok(number),
            _ => Err(self.fail(format_args!("expected an integer in {min}..={max}, got {value}"))),
        }
    }

    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "ranges are checked by `int`")]
    fn write(&mut self, w: &mut Writer, ty: &Ty, env: &[Ty], value: &Value) -> Result<(), Error> {
        match ty {
            Ty::Unit => {}
            Ty::Bool => w.byte(u8::from(value.as_bool().ok_or_else(|| self.fail("expected a boolean"))?)),
            Ty::U8 => w.byte(self.int(value, 0, 255)? as u8),
            Ty::I8 => w.byte(self.int(value, -128, 127)? as i8 as u8),
            Ty::U16 => w.varint(self.int(value, 0, u16::MAX.into())? as u64),
            Ty::U32 => w.varint(self.int(value, 0, u32::MAX.into())? as u64),
            Ty::U64 => w.varint(self.int(value, 0, u64::MAX.into())? as u64),
            Ty::I16 => w.zigzag(self.int(value, i16::MIN.into(), i16::MAX.into())? as i64),
            Ty::I32 => w.zigzag(self.int(value, i32::MIN.into(), i32::MAX.into())? as i64),
            Ty::I64 => w.zigzag(self.int(value, i64::MIN.into(), i64::MAX.into())? as i64),
            Ty::F32 => w.bytes(&(value.as_f64().ok_or_else(|| self.fail("expected a number"))? as f32).to_le_bytes()),
            Ty::F64 => w.bytes(&value.as_f64().ok_or_else(|| self.fail("expected a number"))?.to_le_bytes()),
            Ty::Str => w.len_prefixed(value.as_str().ok_or_else(|| self.fail("expected a string"))?.as_bytes()),
            Ty::Bytes => w.len_prefixed(&value.as_str().and_then(parse_hex).ok_or_else(|| self.fail("expected a hex string"))?),
            Ty::Uuid => w.bytes(&value.as_str().and_then(parse_uuid).ok_or_else(|| self.fail("expected a hyphenated uuid"))?),
            Ty::Timestamp => {
                let text = value.as_str().ok_or_else(|| self.fail("expected an RFC 3339 timestamp"))?;
                let at = OffsetDateTime::parse(text, &Rfc3339).map_err(|error| self.fail(error))?;
                w.zigzag(at.unix_timestamp_nanos().div_euclid(1_000_000) as i64);
            }
            Ty::Option { of } => {
                w.byte(u8::from(!value.is_null()));
                if !value.is_null() {
                    self.write(w, of, env, value)?;
                }
            }
            Ty::Seq { of } | Ty::Set { of } => {
                let items = value.as_array().ok_or_else(|| self.fail("expected an array"))?;
                w.varint(items.len() as u64);
                self.write_items(w, of, env, items)?;
            }
            Ty::Array { of, len } => {
                let items = value.as_array().filter(|items| items.len() == *len);
                let items = items.ok_or_else(|| self.fail(format_args!("expected an array of {len}")))?;
                self.write_items(w, of, env, items)?;
            }
            Ty::Tuple { of } => {
                let items = value.as_array().filter(|items| items.len() == of.len());
                let items = items.ok_or_else(|| self.fail(format_args!("expected an array of {}", of.len())))?;
                for (index, (ty, item)) in of.iter().zip(items).enumerate() {
                    self.scoped(format!("[{index}]"), |cursor| cursor.write(w, ty, env, item))?;
                }
            }
            Ty::Map { key, value: value_ty } => self.write_map(w, key, value_ty, env, value)?,
            Ty::Named { name, args } => {
                let args: Vec<Ty> = args.iter().map(|ty| ty.substitute(env)).collect();
                self.write_named(w, name, &args, value)?;
            }
            Ty::Param { index } => {
                let ty = env.get(*index).ok_or_else(|| self.fail(format_args!("unbound parameter ${index}")))?;
                self.write(w, ty, &[], value)?;
            }
        }
        Ok(())
    }

    fn write_items(&mut self, w: &mut Writer, ty: &Ty, env: &[Ty], items: &[Value]) -> Result<(), Error> {
        for (index, item) in items.iter().enumerate() {
            self.scoped(format!("[{index}]"), |cursor| cursor.write(w, ty, env, item))?;
        }
        Ok(())
    }

    fn write_map(&mut self, w: &mut Writer, key: &Ty, value_ty: &Ty, env: &[Ty], value: &Value) -> Result<(), Error> {
        if let Value::Object(entries) = value {
            if key.substitute(env) != Ty::Str {
                return Err(self.fail("only maps with string keys are objects; use [[key, value], ...]"));
            }
            w.varint(entries.len() as u64);
            for (name, item) in entries {
                w.len_prefixed(name.as_bytes());
                self.scoped(format!(".{name}"), |cursor| cursor.write(w, value_ty, env, item))?;
            }
            return Ok(());
        }
        let pairs = value.as_array().ok_or_else(|| self.fail("expected an object or an array of [key, value]"))?;
        w.varint(pairs.len() as u64);
        for (index, pair) in pairs.iter().enumerate() {
            self.scoped(format!("[{index}]"), |cursor| match pair.as_array().map(Vec::as_slice) {
                Some([k, v]) => {
                    cursor.write(w, key, env, k)?;
                    cursor.write(w, value_ty, env, v)
                }
                _ => Err(cursor.fail("expected [key, value]")),
            })?;
        }
        Ok(())
    }

    fn write_named(&mut self, w: &mut Writer, name: &str, args: &[Ty], value: &Value) -> Result<(), Error> {
        let def = self.def(name)?;
        match &def.shape {
            Shape::Newtype { ty } => self.write(w, ty, args, value),
            Shape::Struct { fields } => self.write_fields(w, fields, args, value),
            Shape::Enum { variants, other, .. } => {
                let (variant_name, payload) = match value {
                    Value::String(variant) => (variant.as_str(), None),
                    Value::Object(entries) if entries.len() == 1 => {
                        entries.iter().next().map(|(k, v)| (k.as_str(), Some(v))).unwrap_or_default()
                    }
                    _ => return Err(self.fail(format_args!("expected a `{name}` variant name or {{\"Variant\": payload}}"))),
                };
                let (index, variant) = variant_indexes(variants)
                    .zip(variants)
                    .find(|(_, variant)| variant.name == variant_name)
                    .ok_or_else(|| self.fail(format_args!("`{variant_name}` is not a variant of `{name}`")))?;
                let is_other = other.and_then(|other| variants.get(other)).is_some_and(|other| other.name == variant_name);
                self.scoped(format!(".{variant_name}"), |cursor| match (&variant.payload, payload) {
                    (Payload::Newtype { ty }, Some(payload)) if is_other => {
                        let max = match ty {
                            Ty::U8 => u8::MAX.into(),
                            Ty::U16 => u16::MAX.into(),
                            _ => u32::MAX.into(),
                        };
                        w.tag(cursor.int(payload, 0, max)? as u32, false);
                        Ok(())
                    }
                    (Payload::Unit, None | Some(Value::Null)) => {
                        w.tag(index, false);
                        Ok(())
                    }
                    (Payload::Newtype { ty }, Some(payload)) => {
                        w.tag(index, true);
                        let frame = w.begin(0);
                        cursor.write(w, ty, args, payload)?;
                        w.end(frame);
                        Ok(())
                    }
                    (Payload::Fields { fields }, Some(payload)) => {
                        w.tag(index, true);
                        cursor.write_fields(w, fields, args, payload)
                    }
                    (Payload::Unit, Some(_)) => Err(cursor.fail("unit variant takes no payload")),
                    (_, None) => Err(cursor.fail("variant needs a payload")),
                })
            }
        }
    }

    fn write_fields(&mut self, w: &mut Writer, fields: &Fields, env: &[Ty], value: &Value) -> Result<(), Error> {
        let slice = fields.as_slice();
        let values: Vec<Option<&Value>> = match (fields, value) {
            (Fields::Named(_), Value::Object(entries)) => {
                if let Some(unknown) = entries.keys().find(|key| !slice.iter().any(|field| field.name == **key)) {
                    return Err(self.fail(format_args!("unknown field `{unknown}`")));
                }
                slice.iter().map(|field| entries.get(&field.name)).collect()
            }
            (Fields::Tuple(_), Value::Array(items)) if items.len() == slice.len() => items.iter().map(Some).collect(),
            (Fields::Named(_), _) => return Err(self.fail("expected an object")),
            (Fields::Tuple(_), _) => return Err(self.fail(format_args!("expected an array of {}", slice.len()))),
        };
        let mut mask = 0u64;
        for ((bit, _), value) in optional_bits(slice).zip(&values) {
            if let (Some(bit), Some(_)) = (bit, value) {
                mask |= 1u64 << bit;
            }
        }
        let frame = w.begin(mask);
        for (index, (field, value)) in slice.iter().zip(&values).enumerate() {
            let segment = if field.name.is_empty() { format!("[{index}]") } else { format!(".{}", field.name) };
            match value {
                Some(value) => self.scoped(segment, |cursor| cursor.write(w, &field.ty, env, value))?,
                None if field.optional => {}
                None => return Err(self.fail(format_args!("missing field `{}`", field.name))),
            }
        }
        w.end(frame);
        Ok(())
    }

    fn read(&mut self, r: &mut Reader<'_>, ty: &Ty, env: &[Ty]) -> Result<Value, Error> {
        let wire = |cursor: &Self, error: crate::Error| cursor.fail(error);
        Ok(match ty {
            Ty::Unit => Value::Null,
            Ty::Bool => match r.byte().map_err(|e| wire(self, e))? {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                byte => return Err(self.fail(crate::Error::InvalidBool(byte))),
            },
            Ty::U8 => Value::from(r.byte().map_err(|e| wire(self, e))?),
            Ty::I8 => Value::from(r.byte().map_err(|e| wire(self, e))?.cast_signed()),
            Ty::U16 | Ty::U32 | Ty::U64 => Value::from(r.varint().map_err(|e| wire(self, e))?),
            Ty::I16 | Ty::I32 | Ty::I64 => Value::from(r.zigzag().map_err(|e| wire(self, e))?),
            Ty::F32 => {
                Number::from_f64(f32::from_le_bytes(r.array().map_err(|e| wire(self, e))?).into()).map_or(Value::Null, Value::Number)
            }
            Ty::F64 => Number::from_f64(f64::from_le_bytes(r.array().map_err(|e| wire(self, e))?)).map_or(Value::Null, Value::Number),
            Ty::Str => {
                let bytes = r.len_prefixed().map_err(|e| wire(self, e))?;
                Value::String(std::str::from_utf8(bytes).map_err(|_| self.fail(crate::Error::InvalidUtf8))?.to_owned())
            }
            Ty::Bytes => Value::String(hex(r.len_prefixed().map_err(|e| wire(self, e))?)),
            Ty::Uuid => Value::String(hex_uuid(r.array().map_err(|e| wire(self, e))?)),
            Ty::Timestamp => {
                let millis = r.zigzag().map_err(|e| wire(self, e))?;
                let at = OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
                    .map_err(|_| self.fail(crate::Error::InvalidTimestamp))?;
                Value::String(at.format(&Rfc3339).map_err(|error| self.fail(error))?)
            }
            Ty::Option { of } => match r.byte().map_err(|e| wire(self, e))? {
                0 => Value::Null,
                1 => self.read(r, of, env)?,
                byte => return Err(self.fail(crate::Error::InvalidBool(byte))),
            },
            Ty::Seq { of } | Ty::Set { of } => {
                let count = r.count().map_err(|e| wire(self, e))?;
                Value::Array(self.read_items(r, of, env, count)?)
            }
            Ty::Array { of, len } => Value::Array(self.read_items(r, of, env, *len)?),
            Ty::Tuple { of } => {
                let mut items = Vec::with_capacity(of.len());
                for (index, ty) in of.iter().enumerate() {
                    items.push(self.scoped(format!("[{index}]"), |cursor| cursor.read(r, ty, env))?);
                }
                Value::Array(items)
            }
            Ty::Map { key, value } => {
                let count = r.count().map_err(|e| wire(self, e))?;
                if key.substitute(env) == Ty::Str {
                    let mut entries = Map::new();
                    for _ in 0..count {
                        let Value::String(name) = self.read(r, key, env)? else { unreachable!("str decodes to a string") };
                        let item = self.scoped(format!(".{name}"), |cursor| cursor.read(r, value, env))?;
                        entries.insert(name, item);
                    }
                    Value::Object(entries)
                } else {
                    let mut pairs = Vec::with_capacity(count);
                    for index in 0..count {
                        pairs.push(self.scoped(format!("[{index}]"), |cursor| {
                            Ok(Value::Array(vec![cursor.read(r, key, env)?, cursor.read(r, value, env)?]))
                        })?);
                    }
                    Value::Array(pairs)
                }
            }
            Ty::Named { name, args } => {
                let args: Vec<Ty> = args.iter().map(|ty| ty.substitute(env)).collect();
                self.read_named(r, name, &args)?
            }
            Ty::Param { index } => {
                let ty = env.get(*index).ok_or_else(|| self.fail(format_args!("unbound parameter ${index}")))?;
                self.read(r, ty, &[])?
            }
        })
    }

    fn read_items(&mut self, r: &mut Reader<'_>, ty: &Ty, env: &[Ty], count: usize) -> Result<Vec<Value>, Error> {
        let mut items = Vec::with_capacity(count.min(r.remaining()));
        for index in 0..count {
            items.push(self.scoped(format!("[{index}]"), |cursor| cursor.read(r, ty, env))?);
        }
        Ok(items)
    }

    fn read_named(&mut self, r: &mut Reader<'_>, name: &str, args: &[Ty]) -> Result<Value, Error> {
        let def = self.def(name)?;
        match &def.shape {
            Shape::Newtype { ty } => self.read(r, ty, args),
            Shape::Struct { fields } => self.read_fields(r, fields, args),
            Shape::Enum { variants, other, .. } => {
                let (index, has_payload) = r.tag().map_err(|error| self.fail(error))?;
                let holds_index = |position: usize| Some(position) == *other && variants[position].payload != Payload::Unit;
                let position = variant_indexes(variants).enumerate().position(|(position, known)| known == index && !holds_index(position));
                let Some(variant) = position.map(|position| &variants[position]) else {
                    if has_payload {
                        r.skip_frame().map_err(|error| self.fail(error))?;
                    }
                    return match other.and_then(|other| variants.get(other)) {
                        Some(fallback) if fallback.payload == Payload::Unit => Ok(Value::String(fallback.name.clone())),
                        Some(fallback) => Ok(Value::Object(Map::from_iter([(fallback.name.clone(), Value::from(index))]))),
                        None => Err(self.fail(format_args!("`{name}` has no variant {index}"))),
                    };
                };
                self.scoped(format!(".{}", variant.name), |cursor| {
                    let payload = match (&variant.payload, has_payload) {
                        (Payload::Unit, false) => return Ok(Value::String(variant.name.clone())),
                        (Payload::Newtype { ty }, true) => {
                            let (mut body, _) = r.frame().map_err(|error| cursor.fail(error))?;
                            cursor.read(&mut body, ty, args)?
                        }
                        (Payload::Fields { fields }, true) => cursor.read_fields(r, fields, args)?,
                        (Payload::Unit, true) => return Err(cursor.fail("unexpected payload on a unit variant")),
                        (_, false) => return Err(cursor.fail("missing payload")),
                    };
                    Ok(Value::Object(Map::from_iter([(variant.name.clone(), payload)])))
                })
            }
        }
    }

    fn read_fields(&mut self, r: &mut Reader<'_>, fields: &Fields, env: &[Ty]) -> Result<Value, Error> {
        let (mut body, mask) = r.frame().map_err(|error| self.fail(error))?;
        let slice = fields.as_slice();
        let mut named = Map::new();
        let mut items = Vec::with_capacity(slice.len());
        for (index, (bit, field)) in optional_bits(slice).enumerate() {
            let segment = if field.name.is_empty() { format!("[{index}]") } else { format!(".{}", field.name) };
            let value = match bit {
                Some(bit) if mask & (1u64 << bit) == 0 => None,
                _ if field.default && body.is_empty() => Some(zero_value(&field.ty.substitute(env))),
                _ => Some(self.scoped(segment, |cursor| cursor.read(&mut body, &field.ty, env))?),
            };
            match fields {
                Fields::Named(_) => {
                    if let Some(value) = value {
                        named.insert(field.name.clone(), value);
                    }
                }
                Fields::Tuple(_) => items.push(value.unwrap_or(Value::Null)),
            }
        }
        Ok(match fields {
            Fields::Named(_) => Value::Object(named),
            Fields::Tuple(_) => Value::Array(items),
        })
    }
}
