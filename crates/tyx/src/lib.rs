mod codec;
#[cfg(feature = "json")]
pub mod json;
mod schema;
mod wire;

pub use codec::{Decode, Encode, Unknown};
#[cfg(feature = "export")]
pub use schema::schema;
pub use schema::{Param, Registry, Type};
pub use tyx_derive::Type;
pub use tyx_ir as ir;
pub use wire::{Error, Frame, Reader, Writer};

#[must_use]
pub fn to_vec<T: Encode + ?Sized>(value: &T) -> Vec<u8> {
    let mut w = Writer::new();
    value.encode(&mut w);
    w.into_vec()
}

/// Appends the encoding of `value` to `buf`, reusing its capacity.
pub fn encode_into<T: Encode + ?Sized>(value: &T, buf: &mut Vec<u8>) {
    let mut w = Writer::from(std::mem::take(buf));
    value.encode(&mut w);
    *buf = w.into_vec();
}

pub fn from_slice<'de, T: Decode<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    let mut r = Reader::new(bytes);
    let value = T::decode(&mut r)?;
    if r.is_empty() { Ok(value) } else { Err(Error::TrailingBytes) }
}

#[doc(hidden)]
pub mod __private {
    #[cfg(feature = "export")]
    pub use crate::schema::Export;
    #[cfg(feature = "export")]
    pub use inventory;
}

#[cfg(feature = "export")]
#[doc(hidden)]
#[macro_export]
macro_rules! __export {
    ($describe:expr) => {
        $crate::__private::inventory::submit! { $crate::__private::Export($describe) }
    };
}

#[cfg(not(feature = "export"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __export {
    ($describe:expr) => {};
}
