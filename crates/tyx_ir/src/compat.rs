use std::collections::HashSet;
use std::fmt;

use crate::{Def, Field, Fields, Payload, Schema, Shape, Ty, variant_indexes};

/// A change from a frozen schema that an old peer cannot survive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Break {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Break {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Every change from `old` to `new` that breaks an old reader or writer of a type defined in both, by the evolution
/// rules of SPEC.md. Type names are not on the wire, so a type that disappears breaks only the types referring to it;
/// field and variant names are in JSON, so renaming those breaks. Use [`Compat`] to diff from roots of your own.
#[must_use]
pub fn compat(old: &Schema, new: &Schema) -> Vec<Break> {
    let mut diff = Compat::new(old, new);
    for def in old.types.iter().filter(|def| new.def(&def.name).is_some()) {
        let args = (0..def.params.len()).map(|index| Ty::Param { index }).collect();
        let ty = Ty::Named { name: def.name.clone(), args };
        diff.ty(&def.name, &ty, &ty);
    }
    diff.into_breaks()
}

pub struct Compat<'s> {
    old: &'s Schema,
    new: &'s Schema,
    seen: HashSet<(String, String)>,
    breaks: Vec<Break>,
}

impl<'s> Compat<'s> {
    #[must_use]
    pub fn new(old: &'s Schema, new: &'s Schema) -> Self {
        Self { old, new, seen: HashSet::new(), breaks: Vec::new() }
    }

    #[must_use]
    pub fn into_breaks(self) -> Vec<Break> {
        self.breaks
    }

    pub fn fail(&mut self, path: &str, message: impl Into<String>) {
        self.breaks.push(Break { path: path.to_owned(), message: message.into() });
    }

    /// Diffs `old` (a type of the old schema) against `new` (a type of the new one); each named pair is diffed once.
    pub fn ty(&mut self, path: &str, old: &Ty, new: &Ty) {
        let (old, new) = (resolve(self.old, old), resolve(self.new, new));
        match (&old, &new) {
            (Ty::Option { of: a }, Ty::Option { of: b })
            | (Ty::Seq { of: a }, Ty::Seq { of: b })
            | (Ty::Set { of: a }, Ty::Set { of: b }) => {
                self.ty(path, a, b);
            }
            (Ty::Map { key: ak, value: av }, Ty::Map { key: bk, value: bv }) => {
                self.ty(&format!("{path} key"), ak, bk);
                self.ty(&format!("{path} value"), av, bv);
            }
            (Ty::Array { of: a, len: al }, Ty::Array { of: b, len: bl }) => {
                if al != bl {
                    self.fail(path, format!("array length changed from {al} to {bl}"));
                }
                self.ty(path, a, b);
            }
            (Ty::Tuple { of: a }, Ty::Tuple { of: b }) => {
                if a.len() != b.len() {
                    self.fail(path, format!("tuple length changed from {} to {}", a.len(), b.len()));
                }
                for (index, (a, b)) in a.iter().zip(b).enumerate() {
                    self.ty(&format!("{path}.{index}"), a, b);
                }
            }
            (Ty::Named { name: an, args: aa }, Ty::Named { name: bn, args: ba }) => {
                if !self.seen.insert((old.to_string(), new.to_string())) {
                    return;
                }
                let (Some(a), Some(b)) = (self.old.def(an), self.new.def(bn)) else {
                    self.fail(path, format!("refers to an undefined type `{an}` or `{bn}`"));
                    return;
                };
                self.def(&format!("{path} {bn}"), a, aa, b, ba);
            }
            _ if old == new => {}
            _ => self.fail(path, format!("retyped from `{old}` to `{new}`")),
        }
    }

    fn def(&mut self, path: &str, old: &Def, old_args: &[Ty], new: &Def, new_args: &[Ty]) {
        match (&old.shape, &new.shape) {
            (Shape::Struct { fields: a }, Shape::Struct { fields: b }) => self.fields(path, a, old_args, b, new_args),
            (Shape::Enum { variants: a, tag: at, other: ao }, Shape::Enum { variants: b, tag: bt, other: bo }) => {
                if at != bt {
                    self.fail(path, format!("JSON tag changed from {at:?} to {bt:?}"));
                }
                if ao != bo && ao.is_some() {
                    self.fail(path, "moved or dropped its `other` variant");
                }
                let after_indexes: Vec<u32> = variant_indexes(b).collect();
                let before_indexes: Vec<u32> = variant_indexes(a).collect();
                for (index, before) in before_indexes.iter().zip(a) {
                    let at = format!("{path}::{}", before.name);
                    let Some(after) = after_indexes.iter().position(|after| after == index).map(|position| &b[position]) else {
                        self.fail(&at, "removed");
                        continue;
                    };
                    if after.name != before.name {
                        self.fail(&at, format!("renamed or reordered to `{}`", after.name));
                    }
                    match (&before.payload, &after.payload) {
                        (Payload::Unit, Payload::Unit) => {}
                        (Payload::Newtype { ty: x }, Payload::Newtype { ty: y }) => {
                            self.ty(&at, &x.substitute(old_args), &y.substitute(new_args))
                        }
                        (Payload::Fields { fields: x }, Payload::Fields { fields: y }) => self.fields(&at, x, old_args, y, new_args),
                        _ => self.fail(&at, "payload kind changed"),
                    }
                }
                let added = after_indexes.iter().zip(b).find(|(index, _)| !before_indexes.contains(index));
                if let Some((_, added)) = added
                    && ao.is_none()
                {
                    self.fail(path, format!("added `{}`, but old readers have no `other` variant", added.name));
                }
            }
            _ => self.fail(path, "changed between struct, enum and newtype"),
        }
    }

    fn fields(&mut self, path: &str, old: &Fields, old_args: &[Ty], new: &Fields, new_args: &[Ty]) {
        let (a, b) = (old.as_slice(), new.as_slice());
        for (index, before) in a.iter().enumerate() {
            let at = format!("{path}.{}", label(before, index));
            let Some(after) = b.get(index) else {
                self.fail(&at, "removed");
                continue;
            };
            if after.name != before.name {
                self.fail(&at, format!("renamed or reordered to `{}`", label(after, index)));
            }
            if after.optional != before.optional || after.default != before.default {
                self.fail(&at, "changed between required, optional and default");
            }
            self.ty(&at, &before.ty.substitute(old_args), &after.ty.substitute(new_args));
        }
        for (index, after) in b.iter().enumerate().skip(a.len()) {
            if !after.optional && !after.default {
                self.fail(&format!("{path}.{}", label(after, index)), "appended without being optional or default");
            }
        }
    }
}

fn label(field: &Field, index: usize) -> String {
    if field.name.is_empty() { index.to_string() } else { field.name.clone() }
}

/// Newtypes are transparent on the wire and in JSON; `Seq<u8>` is how schemas before `Bytes` spelled it.
fn resolve(schema: &Schema, ty: &Ty) -> Ty {
    if let Ty::Seq { of } = ty
        && **of == Ty::U8
    {
        return Ty::Bytes;
    }
    if let Ty::Named { name, args } = ty
        && let Some(Def { shape: Shape::Newtype { ty: inner }, .. }) = schema.def(name)
    {
        return resolve(schema, &inner.substitute(args));
    }
    ty.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Variant;

    fn field(name: &str, ty: Ty, optional: bool, default: bool) -> Field {
        Field { name: name.to_owned(), ty, optional, default }
    }

    fn named(name: &str) -> Ty {
        Ty::Named { name: name.to_owned(), args: Vec::new() }
    }

    fn schema(types: Vec<Def>) -> Schema {
        Schema { types }
    }

    fn strukt(fields: Vec<Field>) -> Schema {
        schema(vec![Def {
            name: "T".to_owned(),
            module: "m".to_owned(),
            params: Vec::new(),
            shape: Shape::Struct { fields: Fields::Named(fields) },
        }])
    }

    fn enumeration(names: &[&str], other: Option<usize>) -> Schema {
        let variants = names.iter().map(|name| Variant { name: (*name).to_owned(), index: None, payload: Payload::Unit }).collect();
        schema(vec![Def {
            name: "T".to_owned(),
            module: "m".to_owned(),
            params: Vec::new(),
            shape: Shape::Enum { variants, tag: None, other },
        }])
    }

    #[test]
    fn struct_rules() {
        let base = strukt(vec![field("a", Ty::U32, false, false)]);
        assert!(compat(&base, &strukt(vec![field("a", Ty::U32, false, false), field("b", Ty::Str, true, false)])).is_empty());
        assert!(compat(&base, &strukt(vec![field("a", Ty::U32, false, false), field("b", Ty::Str, false, true)])).is_empty());
        assert_eq!(compat(&base, &strukt(vec![field("a", Ty::U32, false, false), field("b", Ty::Str, false, false)])).len(), 1);
        assert_eq!(compat(&base, &strukt(vec![])).len(), 1);
        assert_eq!(compat(&base, &strukt(vec![field("a", Ty::I32, false, false)])).len(), 1);
        assert_eq!(compat(&base, &strukt(vec![field("z", Ty::U32, false, false)])).len(), 1);
        assert_eq!(compat(&base, &strukt(vec![field("a", Ty::U32, true, false)])).len(), 1);
    }

    #[test]
    fn enum_rules() {
        assert!(compat(&enumeration(&["A", "Other"], Some(1)), &enumeration(&["A", "Other", "B"], Some(1))).is_empty());
        assert_eq!(compat(&enumeration(&["A"], None), &enumeration(&["A", "B"], None)).len(), 1);
        assert_eq!(compat(&enumeration(&["A", "B"], None), &enumeration(&["B", "A"], None)).len(), 2);
        assert_eq!(compat(&enumeration(&["A", "B"], None), &enumeration(&["A"], None)).len(), 1);
    }

    #[test]
    fn newtypes_and_renames_are_transparent() {
        let id = Def { name: "Id".to_owned(), module: "m".to_owned(), params: Vec::new(), shape: Shape::Newtype { ty: Ty::Uuid } };
        let wrapped = schema(vec![
            id,
            Def {
                name: "T".to_owned(),
                module: "m".to_owned(),
                params: Vec::new(),
                shape: Shape::Struct { fields: Fields::Named(vec![field("id", named("Id"), false, false)]) },
            },
        ]);
        assert!(compat(&wrapped, &strukt(vec![field("id", Ty::Uuid, false, false)])).is_empty());
    }
}
