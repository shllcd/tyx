use crate::wire::{Error, Reader, Writer};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, Hash};
use std::rc::Rc;
use std::sync::Arc;

pub trait Encode {
    fn encode(&self, w: &mut Writer);

    #[doc(hidden)]
    #[inline]
    fn encode_slice(items: &[Self], w: &mut Writer)
    where
        Self: Sized,
    {
        for item in items {
            item.encode(w);
        }
    }
}

pub trait Decode<'de>: Sized {
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error>;

    #[doc(hidden)]
    #[inline]
    fn decode_vec(r: &mut Reader<'de>, count: usize) -> Result<Vec<Self>, Error> {
        let mut items = Vec::with_capacity(count.min(r.remaining()));
        for _ in 0..count {
            items.push(Self::decode(r)?);
        }
        Ok(items)
    }
}

/// The optional-field mask bits and trailing frame bytes of fields a newer schema appended, kept by a
/// `#[tyx(unknown)]` field so that re-encoding a decoded value writes them back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Unknown {
    pub mask: u64,
    pub bytes: Vec<u8>,
}

impl Unknown {
    /// Takes the rest of a frame body whose first `known_optional` mask bits belong to known fields.
    pub fn rest(body: &mut Reader<'_>, mask: u64, known_optional: u32) -> Result<Self, Error> {
        let mask = mask & u64::MAX.checked_shl(known_optional).unwrap_or(0);
        Ok(Self { mask, bytes: body.take(body.remaining())?.to_vec() })
    }
}

impl Encode for () {
    #[inline]
    fn encode(&self, _: &mut Writer) {}
}

impl Decode<'_> for () {
    #[inline]
    fn decode(_: &mut Reader<'_>) -> Result<Self, Error> {
        Ok(())
    }
}

impl Encode for bool {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.byte(u8::from(*self));
    }
}

impl Decode<'_> for bool {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        match r.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            byte => Err(Error::InvalidBool(byte)),
        }
    }
}

impl Encode for u8 {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.byte(*self);
    }

    #[inline]
    fn encode_slice(items: &[Self], w: &mut Writer) {
        w.bytes(items);
    }
}

impl Decode<'_> for u8 {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        r.byte()
    }

    #[inline]
    fn decode_vec(r: &mut Reader<'_>, count: usize) -> Result<Vec<Self>, Error> {
        r.take(count).map(<[u8]>::to_vec)
    }
}

impl Encode for i8 {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.byte(*self as u8);
    }
}

impl Decode<'_> for i8 {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        r.byte().map(|byte| byte as i8)
    }
}

macro_rules! unsigned {
    ($($ty:ty),+) => {$(
        impl Encode for $ty {
            #[inline]
            fn encode(&self, w: &mut Writer) {
                w.varint(*self as u64);
            }
        }

        impl Decode<'_> for $ty {
            #[inline]
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                <$ty>::try_from(r.varint()?).map_err(|_| Error::VarintOverflow)
            }
        }
    )+};
}

macro_rules! signed {
    ($($ty:ty),+) => {$(
        impl Encode for $ty {
            #[inline]
            fn encode(&self, w: &mut Writer) {
                w.zigzag(*self as i64);
            }
        }

        impl Decode<'_> for $ty {
            #[inline]
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                <$ty>::try_from(r.zigzag()?).map_err(|_| Error::VarintOverflow)
            }
        }
    )+};
}

unsigned!(u16, u32, u64, usize);
signed!(i16, i32, i64, isize);

macro_rules! float {
    ($($ty:ty),+) => {$(
        impl Encode for $ty {
            #[inline]
            fn encode(&self, w: &mut Writer) {
                w.bytes(&self.to_le_bytes());
            }
        }

        impl Decode<'_> for $ty {
            #[inline]
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                r.array().map(<$ty>::from_le_bytes)
            }
        }
    )+};
}

float!(f32, f64);

impl Encode for str {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.len_prefixed(self.as_bytes());
    }
}

impl<'de: 'a, 'a> Decode<'de> for &'a str {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        std::str::from_utf8(r.len_prefixed()?).map_err(|_| Error::InvalidUtf8)
    }
}

impl<'de: 'a, 'a> Decode<'de> for &'a [u8] {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        r.len_prefixed()
    }
}

impl<'de: 'a, 'a> Decode<'de> for Cow<'a, str> {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        <&str>::decode(r).map(Cow::Borrowed)
    }
}

impl Encode for String {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        self.as_str().encode(w);
    }
}

impl Decode<'_> for String {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        <&str>::decode(r).map(str::to_owned)
    }
}

macro_rules! shared_str {
    ($($ptr:ident),+) => {$(
        impl Decode<'_> for $ptr<str> {
            #[inline]
            fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
                <&str>::decode(r).map(<$ptr<str>>::from)
            }
        }
    )+};
}

shared_str!(Box, Rc, Arc);

macro_rules! pointer {
    ($($ptr:ident),+) => {$(
        impl<T: Encode + ?Sized> Encode for $ptr<T> {
            #[inline]
            fn encode(&self, w: &mut Writer) {
                (**self).encode(w);
            }
        }

        impl<'de, T: Decode<'de>> Decode<'de> for $ptr<T> {
            #[inline]
            fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
                T::decode(r).map($ptr::new)
            }
        }
    )+};
}

pointer!(Box, Rc, Arc);

impl<T: Encode + ?Sized> Encode for &T {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        (**self).encode(w);
    }
}

impl<T: Encode + ToOwned + ?Sized> Encode for Cow<'_, T> {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        (**self).encode(w);
    }
}

impl<T: Encode> Encode for [T] {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.varint(self.len() as u64);
        T::encode_slice(self, w);
    }
}

impl<T: Encode> Encode for Vec<T> {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        self.as_slice().encode(w);
    }
}

impl<'de, T: Decode<'de>> Decode<'de> for Vec<T> {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        let count = r.count()?;
        T::decode_vec(r, count)
    }
}

macro_rules! shared_slice {
    ($($ptr:ident),+) => {$(
        impl<'de, T: Decode<'de>> Decode<'de> for $ptr<[T]> {
            #[inline]
            fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
                Vec::<T>::decode(r).map(<$ptr<[T]>>::from)
            }
        }
    )+};
}

shared_slice!(Box, Rc, Arc);

impl<T: Encode, const N: usize> Encode for [T; N] {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        T::encode_slice(self, w);
    }
}

impl<'de, T: Decode<'de>, const N: usize> Decode<'de> for [T; N] {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        let items = T::decode_vec(r, N)?;
        Ok(items.try_into().unwrap_or_else(|_| unreachable!()))
    }
}

impl<T: Encode> Encode for Option<T> {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        match self {
            None => w.byte(0),
            Some(value) => {
                w.byte(1);
                value.encode(w);
            }
        }
    }
}

impl<'de, T: Decode<'de>> Decode<'de> for Option<T> {
    #[inline]
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        if bool::decode(r)? { T::decode(r).map(Some) } else { Ok(None) }
    }
}

fn encode_iter<'a, T: Encode + 'a>(w: &mut Writer, len: usize, items: impl Iterator<Item = &'a T>) {
    w.varint(len as u64);
    for item in items {
        item.encode(w);
    }
}

fn encode_pairs<'a, K: Encode + 'a, V: Encode + 'a>(w: &mut Writer, len: usize, pairs: impl Iterator<Item = (&'a K, &'a V)>) {
    w.varint(len as u64);
    for (key, value) in pairs {
        key.encode(w);
        value.encode(w);
    }
}

fn decode_iter<'de, T: Decode<'de>, C: FromIterator<T>>(r: &mut Reader<'de>) -> Result<C, Error> {
    let count = r.count()?;
    (0..count).map(|_| T::decode(r)).collect()
}

fn decode_pairs<'de, K: Decode<'de>, V: Decode<'de>, C: FromIterator<(K, V)>>(r: &mut Reader<'de>) -> Result<C, Error> {
    let count = r.count()?;
    (0..count).map(|_| Ok((K::decode(r)?, V::decode(r)?))).collect()
}

impl<T: Encode, S> Encode for HashSet<T, S> {
    fn encode(&self, w: &mut Writer) {
        encode_iter(w, self.len(), self.iter());
    }
}

impl<'de, T: Decode<'de> + Eq + Hash, S: BuildHasher + Default> Decode<'de> for HashSet<T, S> {
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        decode_iter(r)
    }
}

impl<T: Encode> Encode for BTreeSet<T> {
    fn encode(&self, w: &mut Writer) {
        encode_iter(w, self.len(), self.iter());
    }
}

impl<'de, T: Decode<'de> + Ord> Decode<'de> for BTreeSet<T> {
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        decode_iter(r)
    }
}

impl<K: Encode, V: Encode, S> Encode for HashMap<K, V, S> {
    fn encode(&self, w: &mut Writer) {
        encode_pairs(w, self.len(), self.iter());
    }
}

impl<'de, K: Decode<'de> + Eq + Hash, V: Decode<'de>, S: BuildHasher + Default> Decode<'de> for HashMap<K, V, S> {
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        decode_pairs(r)
    }
}

impl<K: Encode, V: Encode> Encode for BTreeMap<K, V> {
    fn encode(&self, w: &mut Writer) {
        encode_pairs(w, self.len(), self.iter());
    }
}

impl<'de, K: Decode<'de> + Ord, V: Decode<'de>> Decode<'de> for BTreeMap<K, V> {
    fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
        decode_pairs(r)
    }
}

macro_rules! tuple {
    ($($name:ident),+) => {
        impl<$($name: Encode),+> Encode for ($($name,)+) {
            #[inline]
            #[allow(non_snake_case, reason = "bindings reuse the type parameter names")]
            fn encode(&self, w: &mut Writer) {
                let ($($name,)+) = self;
                $($name.encode(w);)+
            }
        }

        impl<'de, $($name: Decode<'de>),+> Decode<'de> for ($($name,)+) {
            #[inline]
            fn decode(r: &mut Reader<'de>) -> Result<Self, Error> {
                Ok(($($name::decode(r)?,)+))
            }
        }
    };
}

tuple!(A);
tuple!(A, B);
tuple!(A, B, C);
tuple!(A, B, C, D);
tuple!(A, B, C, D, E);
tuple!(A, B, C, D, E, F);

#[cfg(feature = "uuid")]
impl Encode for uuid::Uuid {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.bytes(self.as_bytes());
    }
}

#[cfg(feature = "uuid")]
impl Decode<'_> for uuid::Uuid {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        r.array().map(Self::from_bytes)
    }
}

#[cfg(feature = "time")]
impl Encode for time::OffsetDateTime {
    #[inline]
    fn encode(&self, w: &mut Writer) {
        w.zigzag(self.unix_timestamp() * 1000 + i64::from(self.millisecond()));
    }
}

#[cfg(feature = "time")]
impl Decode<'_> for time::OffsetDateTime {
    #[inline]
    fn decode(r: &mut Reader<'_>) -> Result<Self, Error> {
        let millis = r.zigzag()?;
        Self::from_unix_timestamp(millis.div_euclid(1000))
            .and_then(|at| at.replace_millisecond(millis.rem_euclid(1000) as u16))
            .map_err(|_| Error::InvalidTimestamp)
    }
}
