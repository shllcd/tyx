use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use tyx_ir::{Def, Error, Schema, Shape, Ty};

pub trait Type {
    fn ty(reg: &mut Registry) -> Ty;
}

pub struct Param<const INDEX: usize>;

impl<const INDEX: usize> Type for Param<INDEX> {
    fn ty(_: &mut Registry) -> Ty {
        Ty::Param { index: INDEX }
    }
}

enum Slot {
    Pending { module: &'static str },
    Done(Def),
}

impl Slot {
    fn module(&self) -> &str {
        match self {
            Self::Pending { module } => module,
            Self::Done(def) => &def.module,
        }
    }
}

#[derive(Default)]
pub struct Registry {
    slots: BTreeMap<&'static str, Slot>,
    errors: Vec<Error>,
}

impl Registry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add<T: Type + ?Sized>(&mut self) -> Ty {
        T::ty(self)
    }

    pub fn define(
        &mut self,
        name: &'static str,
        module: &'static str,
        params: &[&str],
        args: Vec<Ty>,
        build: impl FnOnce(&mut Self) -> Shape,
    ) -> Ty {
        match self.slots.get(name) {
            Some(slot) if slot.module() != module => {
                let error = Error::Conflict { name: name.to_owned(), first: slot.module().to_owned(), second: module.to_owned() };
                self.errors.push(error);
            }
            Some(_) => {}
            None => {
                self.slots.insert(name, Slot::Pending { module });
                let shape = build(self);
                let params = params.iter().map(|&param| param.to_owned()).collect();
                self.slots.insert(name, Slot::Done(Def { name: name.to_owned(), module: module.to_owned(), params, shape }));
            }
        }
        Ty::Named { name: name.to_owned(), args }
    }

    pub fn into_schema(self) -> Result<Schema, Error> {
        if let Some(error) = self.errors.into_iter().next() {
            return Err(error);
        }
        let types = self
            .slots
            .into_values()
            .filter_map(|slot| match slot {
                Slot::Done(def) => Some(def),
                Slot::Pending { .. } => None,
            })
            .collect();
        let schema = Schema { types };
        schema.validate()?;
        Ok(schema)
    }
}

#[cfg(feature = "export")]
pub struct Export(pub fn(&mut Registry) -> Ty);

#[cfg(feature = "export")]
inventory::collect!(Export);

/// Collects every non-generic `#[derive(Type)]` linked into the current binary.
#[cfg(feature = "export")]
pub fn schema() -> Result<Schema, Error> {
    let mut reg = Registry::new();
    for export in inventory::iter::<Export> {
        (export.0)(&mut reg);
    }
    reg.into_schema()
}

macro_rules! leaf {
    ($($ty:ty => $kind:ident),+ $(,)?) => {$(
        impl Type for $ty {
            fn ty(_: &mut Registry) -> Ty {
                Ty::$kind
            }
        }
    )+};
}

leaf! {
    () => Unit, bool => Bool,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, usize => U64,
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, isize => I64,
    f32 => F32, f64 => F64,
    str => Str, String => Str,
}

#[cfg(feature = "uuid")]
leaf!(uuid::Uuid => Uuid);

#[cfg(feature = "time")]
leaf!(time::OffsetDateTime => Timestamp);

macro_rules! transparent {
    ($($ptr:ident),+) => {$(
        impl<T: Type + ?Sized> Type for $ptr<T> {
            fn ty(reg: &mut Registry) -> Ty {
                T::ty(reg)
            }
        }
    )+};
}

transparent!(Box, Rc, Arc);

impl<T: Type + ?Sized> Type for &T {
    fn ty(reg: &mut Registry) -> Ty {
        T::ty(reg)
    }
}

impl<T: Type + ToOwned + ?Sized> Type for Cow<'_, T> {
    fn ty(reg: &mut Registry) -> Ty {
        T::ty(reg)
    }
}

macro_rules! container {
    ($($ty:ty => $kind:ident),+ $(,)?) => {$(
        impl<T: Type> Type for $ty {
            fn ty(reg: &mut Registry) -> Ty {
                Ty::$kind { of: Box::new(T::ty(reg)) }
            }
        }
    )+};
}

container! {
    Option<T> => Option,
    BTreeSet<T> => Set,
}

fn seq(of: Ty) -> Ty {
    if of == Ty::U8 { Ty::Bytes } else { Ty::Seq { of: Box::new(of) } }
}

impl<T: Type> Type for Vec<T> {
    fn ty(reg: &mut Registry) -> Ty {
        seq(T::ty(reg))
    }
}

impl<T: Type> Type for [T] {
    fn ty(reg: &mut Registry) -> Ty {
        seq(T::ty(reg))
    }
}

impl<T: Type, S> Type for HashSet<T, S> {
    fn ty(reg: &mut Registry) -> Ty {
        Ty::Set { of: Box::new(T::ty(reg)) }
    }
}

impl<T: Type, const N: usize> Type for [T; N] {
    fn ty(reg: &mut Registry) -> Ty {
        Ty::Array { of: Box::new(T::ty(reg)), len: N }
    }
}

impl<K: Type, V: Type, S> Type for HashMap<K, V, S> {
    fn ty(reg: &mut Registry) -> Ty {
        Ty::Map { key: Box::new(K::ty(reg)), value: Box::new(V::ty(reg)) }
    }
}

impl<K: Type, V: Type> Type for BTreeMap<K, V> {
    fn ty(reg: &mut Registry) -> Ty {
        Ty::Map { key: Box::new(K::ty(reg)), value: Box::new(V::ty(reg)) }
    }
}

macro_rules! tuple {
    ($($name:ident),+) => {
        impl<$($name: Type),+> Type for ($($name,)+) {
            fn ty(reg: &mut Registry) -> Ty {
                Ty::Tuple { of: vec![$($name::ty(reg)),+] }
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
