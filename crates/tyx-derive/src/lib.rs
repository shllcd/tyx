use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::ext::IdentExt;
use syn::visit_mut::VisitMut;
use syn::{
    Attribute, Data, DeriveInput, Fields, GenericArgument, GenericParam, Generics, Ident, LitStr, Member, PathArguments, Type,
    parse_macro_input, parse_quote,
};

#[proc_macro_derive(Type, attributes(tyx))]
pub fn derive_type(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input).unwrap_or_else(syn::Error::into_compile_error).into()
}

const MAX_OPTIONAL_FIELDS: usize = 64;

#[derive(Default)]
struct ContainerAttrs {
    name: Option<String>,
    tag: Option<String>,
    krate: Option<syn::Path>,
    encode_only: bool,
    is_screaming_snake_case: bool,
}

#[derive(Default)]
struct FieldAttrs {
    default: bool,
    skip: bool,
    unknown: bool,
}

struct FieldInfo {
    member: Member,
    name: String,
    ty: Type,
    optional_inner: Option<Type>,
    attrs: FieldAttrs,
    binding: Ident,
}

impl FieldInfo {
    fn wire_ty(&self) -> &Type {
        self.optional_inner.as_ref().unwrap_or(&self.ty)
    }
}

struct FieldSet {
    is_named: bool,
    fields: Vec<FieldInfo>,
}

impl FieldSet {
    fn wired(&self) -> impl Iterator<Item = &FieldInfo> {
        self.fields.iter().filter(|field| !field.attrs.skip && !field.attrs.unknown)
    }

    fn unknown(&self) -> Option<&FieldInfo> {
        self.fields.iter().find(|field| field.attrs.unknown)
    }

    fn optional_bits(&self) -> impl Iterator<Item = (u32, &FieldInfo)> {
        self.wired().filter(|field| field.optional_inner.is_some()).zip(0..).map(|(field, bit)| (bit, field))
    }
}

enum VariantKind {
    Unit,
    Newtype(Box<Type>),
    Fields(FieldSet),
}

struct VariantInfo {
    ident: Ident,
    kind: VariantKind,
    is_other: bool,
    index: u32,
    is_pinned: bool,
}

impl VariantInfo {
    fn holds_index(&self) -> bool {
        self.is_other && matches!(self.kind, VariantKind::Newtype(_))
    }
}

enum Body {
    Newtype(Box<Type>),
    Struct(FieldSet),
    Enum(Vec<VariantInfo>),
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attrs = container_attrs(&input.attrs)?;
    if let Some(param) = input.generics.const_params().next() {
        return Err(syn::Error::new_spanned(param, "#[derive(Type)] does not support const generics"));
    }
    let body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                let field = &unnamed.unnamed[0];
                reject_field_attrs(&field.attrs)?;
                Body::Newtype(Box::new(field.ty.clone()))
            }
            fields => Body::Struct(field_set(fields)?),
        },
        Data::Enum(data) => {
            let mut variants = Vec::with_capacity(data.variants.len());
            let mut next_index = 0u32;
            for variant in &data.variants {
                let pinned = variant.discriminant.as_ref().map(|(_, expr)| pinned_index(expr)).transpose()?;
                let index = pinned.unwrap_or(next_index);
                next_index = index.wrapping_add(1);
                let is_other = variant_is_other(&variant.attrs)?;
                let kind = match &variant.fields {
                    Fields::Unit => VariantKind::Unit,
                    Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                        reject_field_attrs(&unnamed.unnamed[0].attrs)?;
                        VariantKind::Newtype(Box::new(unnamed.unnamed[0].ty.clone()))
                    }
                    fields => VariantKind::Fields(field_set(fields)?),
                };
                if is_other && matches!(kind, VariantKind::Fields(_)) {
                    return Err(syn::Error::new_spanned(variant, "#[tyx(other)] requires a unit variant or one holding the index"));
                }
                variants.push(VariantInfo { ident: variant.ident.clone(), kind, is_other, index, is_pinned: pinned.is_some() });
            }
            if variants.iter().filter(|variant| variant.is_other).count() > 1 {
                return Err(syn::Error::new_spanned(&input.ident, "only one variant may be #[tyx(other)]"));
            }
            Body::Enum(variants)
        }
        Data::Union(data) => return Err(syn::Error::new_spanned(data.union_token, "#[derive(Type)] does not support unions")),
    };
    if attrs.is_screaming_snake_case && !matches!(body, Body::Enum(_)) {
        return Err(syn::Error::new_spanned(&input.ident, "#[tyx(rename_all)] renames enum variants"));
    }
    let name = attrs.name.clone().unwrap_or_else(|| input.ident.unraw().to_string());
    let describe = expand_type(input, &attrs, &name, &body);
    let encode = expand_encode(input, &body);
    let decode = if attrs.encode_only { TokenStream::new() } else { expand_decode(input, &name, &body) };
    let export = expand_export(input);
    let krate = attrs.krate.map_or_else(|| quote!(::tyx), |path| quote!(#path));
    Ok(quote! {
        const _: () = {
            use #krate as __tyx;
            #describe
            #encode
            #decode
            #export
        };
    })
}

fn container_attrs(attrs: &[Attribute]) -> syn::Result<ContainerAttrs> {
    let mut parsed = ContainerAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("tyx")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                parsed.name = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("tag") {
                parsed.tag = Some(meta.value()?.parse::<LitStr>()?.value());
            } else if meta.path.is_ident("crate") {
                parsed.krate = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("encode_only") {
                parsed.encode_only = true;
            } else if meta.path.is_ident("rename_all") {
                let case = meta.value()?.parse::<LitStr>()?;
                if case.value() != "SCREAMING_SNAKE_CASE" {
                    return Err(syn::Error::new_spanned(case, "the only supported case is `SCREAMING_SNAKE_CASE`"));
                }
                parsed.is_screaming_snake_case = true;
            } else {
                return Err(meta.error("expected `name`, `tag`, `crate`, `encode_only` or `rename_all`"));
            }
            Ok(())
        })?;
    }
    Ok(parsed)
}

fn screaming_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut previous: Option<char> = None;
    for c in name.chars() {
        if c.is_uppercase() && previous.is_some_and(|previous| previous.is_lowercase() || previous.is_ascii_digit()) {
            out.push('_');
        }
        out.extend(c.to_uppercase());
        previous = Some(c);
    }
    out
}

fn pinned_index(expr: &syn::Expr) -> syn::Result<u32> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(int), .. }) => int.base10_parse(),
        _ => Err(syn::Error::new_spanned(expr, "#[derive(Type)] needs an integer literal discriminant")),
    }
}

fn field_attrs(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut parsed = FieldAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("tyx")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                parsed.default = true;
            } else if meta.path.is_ident("skip") {
                parsed.skip = true;
            } else if meta.path.is_ident("unknown") {
                parsed.unknown = true;
            } else {
                return Err(meta.error("expected `default`, `skip` or `unknown`"));
            }
            Ok(())
        })?;
    }
    Ok(parsed)
}

fn reject_field_attrs(attrs: &[Attribute]) -> syn::Result<()> {
    match attrs.iter().find(|attr| attr.path().is_ident("tyx")) {
        Some(attr) => Err(syn::Error::new_spanned(attr, "newtypes are transparent and take no #[tyx] field attributes")),
        None => Ok(()),
    }
}

fn variant_is_other(attrs: &[Attribute]) -> syn::Result<bool> {
    let mut is_other = false;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("tyx")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("other") {
                is_other = true;
                Ok(())
            } else {
                Err(meta.error("expected `other`"))
            }
        })?;
    }
    Ok(is_other)
}

fn field_set(fields: &Fields) -> syn::Result<FieldSet> {
    let mut infos = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let attrs = field_attrs(&field.attrs)?;
        let (member, name) = match &field.ident {
            Some(ident) => (Member::Named(ident.clone()), ident.unraw().to_string()),
            None => (Member::Unnamed(index.into()), String::new()),
        };
        let optional_inner = option_inner(&field.ty).cloned();
        if attrs.default && optional_inner.is_some() {
            return Err(syn::Error::new_spanned(field, "optional fields are already absent when missing; drop #[tyx(default)]"));
        }
        if attrs.unknown && (attrs.default || attrs.skip || index + 1 != fields.len()) {
            return Err(syn::Error::new_spanned(field, "#[tyx(unknown)] goes alone on the last field"));
        }
        infos.push(FieldInfo { member, name, ty: field.ty.clone(), optional_inner, attrs, binding: format_ident!("__field{index}") });
    }
    let set = FieldSet { is_named: matches!(fields, Fields::Named(_)), fields: infos };
    if set.optional_bits().count() > MAX_OPTIONAL_FIELDS {
        return Err(syn::Error::new(Span::call_site(), format!("at most {MAX_OPTIONAL_FIELDS} Option fields fit the presence mask")));
    }
    Ok(set)
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else { return None };
    match args.args.first() {
        Some(GenericArgument::Type(inner)) if args.args.len() == 1 => Some(inner),
        _ => None,
    }
}

fn type_param_names(generics: &Generics) -> Vec<Ident> {
    generics.type_params().map(|param| param.ident.clone()).collect()
}

struct SubstituteParams<'a>(&'a [Ident]);

impl VisitMut for SubstituteParams<'_> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(path) = ty
            && path.qself.is_none()
            && let Some(ident) = path.path.get_ident()
            && let Some(index) = self.0.iter().position(|param| param == ident)
        {
            *ty = parse_quote!(__tyx::Param<#index>);
            return;
        }
        syn::visit_mut::visit_type_mut(self, ty);
    }
}

fn with_bound(generics: &Generics, bound: &TokenStream) -> Generics {
    let mut generics = generics.clone();
    for param in &mut generics.params {
        if let GenericParam::Type(param) = param {
            param.bounds.push(parse_quote!(#bound));
        }
    }
    generics
}

fn expand_type(input: &DeriveInput, attrs: &ContainerAttrs, name: &str, body: &Body) -> TokenStream {
    let ident = &input.ident;
    let params = type_param_names(&input.generics);
    let generics = with_bound(&input.generics, &quote!(__tyx::Type));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let describe = |ty: &Type| {
        let mut ty = ty.clone();
        SubstituteParams(&params).visit_type_mut(&mut ty);
        quote!(<#ty as __tyx::Type>::ty(reg))
    };
    let describe_fields = |set: &FieldSet| {
        let fields = set.wired().map(|field| {
            let name = &field.name;
            let ty = describe(field.wire_ty());
            let optional = field.optional_inner.is_some();
            let default = field.attrs.default;
            quote!(__tyx::ir::Field { name: #name.to_owned(), ty: #ty, optional: #optional, default: #default })
        });
        let style = if set.is_named { quote!(Named) } else { quote!(Tuple) };
        quote!(__tyx::ir::Fields::#style(::std::vec![#(#fields),*]))
    };
    let shape = match body {
        Body::Newtype(ty) => {
            let ty = describe(ty);
            quote!(__tyx::ir::Shape::Newtype { ty: #ty })
        }
        Body::Struct(set) => {
            let fields = describe_fields(set);
            quote!(__tyx::ir::Shape::Struct { fields: #fields })
        }
        Body::Enum(variants) => {
            let entries = variants.iter().map(|variant| {
                let name = variant.ident.unraw().to_string();
                let name = if attrs.is_screaming_snake_case { screaming_snake_case(&name) } else { name };
                let payload = match &variant.kind {
                    VariantKind::Unit => quote!(__tyx::ir::Payload::Unit),
                    VariantKind::Newtype(ty) => {
                        let ty = describe(ty);
                        quote!(__tyx::ir::Payload::Newtype { ty: #ty })
                    }
                    VariantKind::Fields(set) => {
                        let fields = describe_fields(set);
                        quote!(__tyx::ir::Payload::Fields { fields: #fields })
                    }
                };
                let index = if variant.is_pinned {
                    let index = variant.index;
                    quote!(::std::option::Option::Some(#index))
                } else {
                    quote!(::std::option::Option::None)
                };
                quote!(__tyx::ir::Variant { name: #name.to_owned(), index: #index, payload: #payload })
            });
            let tag = match &attrs.tag {
                Some(tag) => quote!(::std::option::Option::Some(#tag.to_owned())),
                None => quote!(::std::option::Option::None),
            };
            let other = match variants.iter().position(|variant| variant.is_other) {
                Some(index) => quote!(::std::option::Option::Some(#index)),
                None => quote!(::std::option::Option::None),
            };
            quote!(__tyx::ir::Shape::Enum { variants: ::std::vec![#(#entries),*], tag: #tag, other: #other })
        }
    };
    let param_names = params.iter().map(Ident::to_string);
    let args = params.iter().map(|param| quote!(<#param as __tyx::Type>::ty(reg)));
    quote! {
        impl #impl_generics __tyx::Type for #ident #ty_generics #where_clause {
            fn ty(reg: &mut __tyx::Registry) -> __tyx::ir::Ty {
                let args = ::std::vec![#(#args),*];
                reg.define(#name, ::std::module_path!(), &[#(#param_names),*], args, |reg| #shape)
            }
        }
    }
}

fn destructure(path: &TokenStream, set: &FieldSet) -> TokenStream {
    let bindings = set.wired().chain(set.unknown()).map(|field| {
        let member = &field.member;
        let binding = &field.binding;
        quote!(#member: #binding)
    });
    quote!(#path { #(#bindings,)* .. })
}

fn encode_fields(set: &FieldSet) -> TokenStream {
    let mut bits: Vec<TokenStream> = set
        .optional_bits()
        .map(|(bit, field)| {
            let binding = &field.binding;
            quote!(| (u64::from(#binding.is_some()) << #bit))
        })
        .collect();
    let unknown = set.unknown().map(|field| {
        let binding = &field.binding;
        bits.push(quote!(| #binding.mask));
        quote!(w.bytes(&#binding.bytes);)
    });
    let writes = set.wired().map(|field| {
        let binding = &field.binding;
        if field.optional_inner.is_some() {
            quote!(if let ::std::option::Option::Some(value) = #binding { __tyx::Encode::encode(value, w); })
        } else {
            quote!(__tyx::Encode::encode(#binding, w);)
        }
    });
    quote! {
        let frame = w.begin(0u64 #(#bits)*);
        #(#writes)*
        #unknown
        w.end(frame);
    }
}

fn expand_encode(input: &DeriveInput, body: &Body) -> TokenStream {
    let ident = &input.ident;
    let generics = with_bound(&input.generics, &quote!(__tyx::Encode));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let encode = match body {
        Body::Newtype(_) => quote!(__tyx::Encode::encode(&self.0, w);),
        Body::Struct(set) => {
            let pattern = destructure(&quote!(Self), set);
            let fields = encode_fields(set);
            quote! {
                let #pattern = self;
                #fields
            }
        }
        Body::Enum(variants) if variants.is_empty() => quote!(match *self {}),
        Body::Enum(variants) => {
            let arms = variants.iter().map(|variant| {
                let variant_ident = &variant.ident;
                let index = variant.index;
                match &variant.kind {
                    _ if variant.holds_index() => quote!(Self::#variant_ident(index) => w.tag(u32::from(*index), false),),
                    VariantKind::Unit => quote!(Self::#variant_ident => w.tag(#index, false),),
                    VariantKind::Newtype(_) => quote! {
                        Self::#variant_ident(value) => {
                            w.tag(#index, true);
                            let frame = w.begin(0);
                            __tyx::Encode::encode(value, w);
                            w.end(frame);
                        }
                    },
                    VariantKind::Fields(set) => {
                        let pattern = destructure(&quote!(Self::#variant_ident), set);
                        let fields = encode_fields(set);
                        quote! {
                            #pattern => {
                                w.tag(#index, true);
                                #fields
                            }
                        }
                    }
                }
            });
            quote!(match self { #(#arms)* })
        }
    };
    quote! {
        impl #impl_generics __tyx::Encode for #ident #ty_generics #where_clause {
            #[inline]
            fn encode(&self, w: &mut __tyx::Writer) {
                #encode
            }
        }
    }
}

fn decode_fields(path: &TokenStream, set: &FieldSet) -> TokenStream {
    let known_optional = set.optional_bits().count() as u32;
    let mask = if known_optional > 0 || set.unknown().is_some() { quote!(mask) } else { quote!(_) };
    let inits = set.fields.iter().map(|field| {
        let member = &field.member;
        let value = if field.attrs.unknown {
            quote!(__tyx::Unknown::rest(r, mask, #known_optional)?)
        } else if field.attrs.skip {
            quote!(::std::default::Default::default())
        } else if let Some(bit) = set.optional_bits().find(|(_, optional)| optional.member == field.member).map(|(bit, _)| bit) {
            quote! {
                if mask & (1u64 << #bit) != 0 {
                    ::std::option::Option::Some(__tyx::Decode::decode(r)?)
                } else {
                    ::std::option::Option::None
                }
            }
        } else if field.attrs.default {
            quote!(if r.is_empty() { ::std::default::Default::default() } else { __tyx::Decode::decode(r)? })
        } else {
            quote!(__tyx::Decode::decode(r)?)
        };
        quote!(#member: #value)
    });
    quote! {
        let (mut body, #mask) = r.frame()?;
        let r = &mut body;
        ::std::result::Result::Ok(#path { #(#inits),* })
    }
}

fn expand_decode(input: &DeriveInput, name: &str, body: &Body) -> TokenStream {
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    let lifetimes: Vec<_> = generics.lifetimes().map(|param| param.lifetime.clone()).collect();
    generics.params.insert(0, parse_quote!('de));
    let where_clause = generics.make_where_clause();
    for lifetime in &lifetimes {
        where_clause.predicates.push(parse_quote!('de: #lifetime));
    }
    let decoded_types: Vec<&Type> = match body {
        Body::Newtype(ty) => vec![ty],
        Body::Struct(set) => set.wired().map(FieldInfo::wire_ty).collect(),
        Body::Enum(variants) => variants
            .iter()
            .flat_map(|variant| match &variant.kind {
                VariantKind::Unit => Vec::new(),
                VariantKind::Newtype(ty) => vec![&**ty],
                VariantKind::Fields(set) => set.wired().map(FieldInfo::wire_ty).collect(),
            })
            .collect(),
    };
    for param in input.generics.type_params() {
        let param = &param.ident;
        where_clause.predicates.push(parse_quote!(#param: __tyx::Decode<'de>));
    }
    for ty in decoded_types.into_iter().filter(|ty| mentions_static(ty)) {
        where_clause.predicates.push(parse_quote!(#ty: __tyx::Decode<'de>));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let decode = match body {
        Body::Newtype(_) => quote!(::std::result::Result::Ok(Self(__tyx::Decode::decode(r)?))),
        Body::Struct(set) => decode_fields(&quote!(Self), set),
        Body::Enum(variants) => {
            let known = || variants.iter().filter(|variant| !variant.holds_index());
            let arms = known().map(|variant| {
                let variant_ident = &variant.ident;
                let tag = u64::from(variant.index) << 1;
                match &variant.kind {
                    VariantKind::Unit => quote!(#tag => ::std::result::Result::Ok(Self::#variant_ident),),
                    VariantKind::Newtype(_) => {
                        let tag = tag | 1;
                        quote! {
                            #tag => {
                                let (mut body, _) = r.frame()?;
                                ::std::result::Result::Ok(Self::#variant_ident(__tyx::Decode::decode(&mut body)?))
                            }
                        }
                    }
                    VariantKind::Fields(set) => {
                        let tag = tag | 1;
                        let fields = decode_fields(&quote!(Self::#variant_ident), set);
                        quote!(#tag => { #fields })
                    }
                }
            });
            let mismatches = known().map(|variant| {
                let index = variant.index;
                if matches!(variant.kind, VariantKind::Unit) {
                    quote!(#index => ::std::result::Result::Err(__tyx::Error::UnexpectedPayload { ty: #name, index }),)
                } else {
                    quote!(#index => ::std::result::Result::Err(__tyx::Error::MissingPayload { ty: #name, index }),)
                }
            });
            let fallback = match variants.iter().find(|variant| variant.is_other) {
                Some(variant) if variant.holds_index() => {
                    let variant_ident = &variant.ident;
                    quote! {
                        ::std::convert::TryFrom::try_from(index)
                            .map(Self::#variant_ident)
                            .map_err(|_| __tyx::Error::UnknownVariant { ty: #name, index })
                    }
                }
                Some(variant) => {
                    let variant_ident = &variant.ident;
                    quote!(::std::result::Result::Ok(Self::#variant_ident))
                }
                None => quote!(::std::result::Result::Err(__tyx::Error::UnknownVariant { ty: #name, index })),
            };
            quote! {
                let tag = r.varint()?;
                match tag {
                    #(#arms)*
                    _ => {
                        let index = u32::try_from(tag >> 1).map_err(|_| __tyx::Error::VarintOverflow)?;
                        match index {
                            #(#mismatches)*
                            _ => {
                                if tag & 1 == 1 {
                                    r.skip_frame()?;
                                }
                                #fallback
                            }
                        }
                    }
                }
            }
        }
    };
    quote! {
        #[allow(private_bounds)]
        impl #impl_generics __tyx::Decode<'de> for #ident #ty_generics #where_clause {
            #[inline]
            fn decode(r: &mut __tyx::Reader<'de>) -> ::std::result::Result<Self, __tyx::Error> {
                #decode
            }
        }
    }
}

fn mentions_static(ty: &Type) -> bool {
    struct Finder(bool);
    impl<'ast> syn::visit::Visit<'ast> for Finder {
        fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
            self.0 |= lifetime.ident == "static";
        }
    }
    let mut finder = Finder(false);
    syn::visit::Visit::visit_type(&mut finder, ty);
    finder.0
}

/// A generic type is exported over `Param<i>`, which yields its generic definition, unless bounds
/// beyond `Type` rule that out; then it reaches the schema only through a non-generic type using it.
fn expand_export(input: &DeriveInput) -> TokenStream {
    let bounded = input.generics.where_clause.is_some() || input.generics.type_params().any(|param| !param.bounds.is_empty());
    if bounded {
        return TokenStream::new();
    }
    let ident = &input.ident;
    let mut index = 0usize;
    let args = input.generics.params.iter().filter_map(|param| match param {
        GenericParam::Lifetime(_) => Some(quote!('static)),
        GenericParam::Type(_) => {
            index += 1;
            let param = index - 1;
            Some(quote!(__tyx::Param<#param>))
        }
        GenericParam::Const(_) => None,
    });
    let args: Vec<TokenStream> = args.collect();
    let ty = if args.is_empty() { quote!(#ident) } else { quote!(#ident<#(#args),*>) };
    quote! {
        __tyx::__export!(<#ty as __tyx::Type>::ty);
    }
}

#[cfg(test)]
mod tests {
    use super::screaming_snake_case;

    #[test]
    fn splits_words_at_case_changes() {
        assert_eq!(screaming_snake_case("Mangalib"), "MANGALIB");
        assert_eq!(screaming_snake_case("CdnlibsApi"), "CDNLIBS_API");
        assert_eq!(screaming_snake_case("Mp4Video"), "MP4_VIDEO");
    }
}
