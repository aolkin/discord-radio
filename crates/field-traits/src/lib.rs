//! Implement getter-style traits from same-named fields.
//!
//! ```ignore
//! use field_traits::{field_impl, field_trait};
//!
//! #[field_trait]
//! pub trait Weighted {
//!     fn weight(&self) -> u32;
//!     fn name(&self) -> &str;
//! }
//!
//! #[field_impl(Weighted)]
//! struct Track { weight: u32, name: String }
//!
//! #[field_impl(Weighted)]
//! enum Entry { Track(Track), Other(OtherTrack) }
//! ```
//!
//! `fn x(&self) -> T` returns `self.x` (so `T` must be `Copy`) and `fn x(&self) -> &T`
//! returns `&self.x`. Methods with default bodies are left alone. On an enum, every
//! variant must hold exactly one unnamed field, and each method reads the field from that
//! inner value.
//!
//! `#[field_trait]` puts a hidden macro beside the trait, under the trait's name, so
//! importing the trait also imports what `#[field_impl]` needs. Types named in the trait's
//! method signatures resolve where `#[field_impl]` is used.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use std::hash::{DefaultHasher, Hash, Hasher};
use syn::punctuated::Punctuated;
use syn::{
    Data, DeriveInput, Error, Fields, FnArg, ItemTrait, Path, ReturnType, Signature, Token,
    TraitItem, Type, parse_macro_input,
};

#[proc_macro_attribute]
pub fn field_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = TokenStream2::from(args);
    let item = parse_macro_input!(input as ItemTrait);
    let mut errors = Vec::new();
    if !args.is_empty() {
        errors.push(Error::new_spanned(
            args,
            "#[field_trait] takes no arguments",
        ));
    }
    if !item.generics.params.is_empty() {
        errors.push(Error::new_spanned(
            &item.generics.params,
            "#[field_trait] does not support generic traits",
        ));
    }

    let mut getters = Vec::new();
    for trait_item in &item.items {
        match trait_item {
            TraitItem::Fn(method) if method.default.is_none() => match getter(&method.sig) {
                Ok(getter) => getters.push(getter),
                Err(error) => errors.push(error),
            },
            TraitItem::Fn(_) => {}
            TraitItem::Const(constant) if constant.default.is_none() => {
                errors.push(Error::new_spanned(
                    constant,
                    "#[field_trait] cannot implement an associated const without a default",
                ))
            }
            TraitItem::Type(ty) if ty.default.is_none() => errors.push(Error::new_spanned(
                ty,
                "#[field_trait] cannot implement an associated type without a default",
            )),
            TraitItem::Const(_) | TraitItem::Type(_) => {}
            other => errors.push(Error::new_spanned(
                other,
                "#[field_trait] does not support this trait item",
            )),
        }
    }

    let errors = errors.into_iter().map(Error::into_compile_error);
    let companion = companion_macro(&item, &getters);
    quote! {
        #item
        #companion
        #(#errors)*
    }
    .into()
}

struct Getter {
    sig: Signature,
    by_ref: bool,
}

fn getter(sig: &Signature) -> Result<Getter, Error> {
    if sig.constness.is_some()
        || sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
    {
        return Err(Error::new_spanned(
            sig,
            "#[field_trait] methods must be plain non-generic `fn`s",
        ));
    }
    let takes_only_ref_self = sig.inputs.len() == 1
        && matches!(
            sig.inputs.first(),
            Some(FnArg::Receiver(receiver))
                if receiver.reference.is_some()
                    && receiver.mutability.is_none()
                    && receiver.colon_token.is_none()
        );
    if !takes_only_ref_self {
        return Err(Error::new_spanned(
            if sig.inputs.is_empty() {
                sig.to_token_stream()
            } else {
                sig.inputs.to_token_stream()
            },
            "#[field_trait] methods must take only `&self`",
        ));
    }
    let by_ref = match &sig.output {
        ReturnType::Default => {
            return Err(Error::new_spanned(
                sig,
                "#[field_trait] methods must return `T` or `&T`",
            ));
        }
        ReturnType::Type(_, ty) => match &**ty {
            Type::Reference(reference) if reference.mutability.is_some() => {
                return Err(Error::new_spanned(
                    ty,
                    "#[field_trait] methods cannot return `&mut T`",
                ));
            }
            Type::Reference(_) => true,
            _ => false,
        },
    };
    Ok(Getter {
        sig: sig.clone(),
        by_ref,
    })
}

fn companion_macro(item: &ItemTrait, getters: &[Getter]) -> TokenStream2 {
    let mut hasher = DefaultHasher::new();
    item.to_token_stream().to_string().hash(&mut hasher);
    let trait_name = &item.ident;
    let macro_name = format_ident!("__field_trait_{}_{:016x}", trait_name, hasher.finish());
    let vis = &item.vis;

    let struct_methods = getters.iter().map(|Getter { sig, by_ref }| {
        let field = &sig.ident;
        let reference = by_ref.then(|| quote!(&));
        quote!(#sig { #reference self.#field })
    });
    let enum_methods = getters.iter().map(|Getter { sig, by_ref }| {
        let field = &sig.ident;
        let reference = by_ref.then(|| quote!(&));
        quote! {
            #sig {
                match self {
                    $(Self::$variant(inner) => #reference inner.#field,)*
                }
            }
        }
    });

    quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_name {
            (
                [$($trait_path:tt)*] [$($impl_generics:tt)*] [$($self_ty:tt)*] [$($where_clause:tt)*]
                struct
            ) => {
                impl $($impl_generics)* $($trait_path)* for $($self_ty)* $($where_clause)* {
                    #(#struct_methods)*
                }
            };
            (
                [$($trait_path:tt)*] [$($impl_generics:tt)*] [$($self_ty:tt)*] [$($where_clause:tt)*]
                enum [$($variant:ident)*]
            ) => {
                impl $($impl_generics)* $($trait_path)* for $($self_ty)* $($where_clause)* {
                    #(#enum_methods)*
                }
            };
        }
        #[doc(hidden)]
        #[allow(unused_imports)]
        #vis use #macro_name as #trait_name;
    }
}

#[proc_macro_attribute]
pub fn field_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let trait_paths = parse_macro_input!(args with Punctuated::<Path, Token![,]>::parse_terminated);
    let original = TokenStream2::from(input.clone());
    let item = parse_macro_input!(input as DeriveInput);

    let shape = match &item.data {
        Data::Struct(_) => Ok(quote!(struct)),
        Data::Enum(data) => {
            let mut errors = data
                .variants
                .iter()
                .filter_map(|variant| match &variant.fields {
                    Fields::Unnamed(fields) if fields.unnamed.len() == 1 => None,
                    _ => Some(Error::new_spanned(
                        variant,
                        "#[field_impl] enum variants must hold exactly one unnamed field",
                    )),
                });
            match errors.next() {
                Some(mut first) => {
                    errors.for_each(|error| first.combine(error));
                    Err(first)
                }
                None => {
                    let variants = data.variants.iter().map(|variant| &variant.ident);
                    Ok(quote!(enum [#(#variants)*]))
                }
            }
        }
        Data::Union(data) => Err(Error::new_spanned(
            data.union_token,
            "#[field_impl] supports only structs and enums",
        )),
    };
    let shape = match shape {
        Ok(shape) => shape,
        Err(error) => {
            let error = error.into_compile_error();
            return quote!(#original #error).into();
        }
    };
    if trait_paths.is_empty() {
        let error = Error::new(Span::call_site(), "#[field_impl] needs at least one trait")
            .into_compile_error();
        return quote!(#original #error).into();
    }

    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let impls = trait_paths.iter().map(|path| {
        quote! {
            #path! {
                [#path] [#impl_generics] [#name #ty_generics] [#where_clause]
                #shape
            }
        }
    });
    quote! {
        #original
        #(#impls)*
    }
    .into()
}
