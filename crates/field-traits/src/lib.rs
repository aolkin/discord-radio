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
//! `fn x(&self) -> T` returns `self.x.clone()` (so `T` must be `Clone`) and `fn x(&self) -> &T`
//! returns `&self.x`. Methods with default bodies are left alone. On an enum, every
//! variant must hold exactly one unnamed field, and each method reads the field from that
//! inner value.
//!
//! On a struct or enum, `#[field_impl]` takes trait names and writes each whole impl. On
//! a hand-written `impl Trait for Type` block, it takes getter names and adds just those
//! methods, which lets a trait with associated consts or types that have no default use
//! it. The impl block form supports structs only, because it cannot see the type definition:
//!
//! ```ignore
//! #[field_impl(name)]
//! impl RegistryEntry for SignalProfile {
//!     const KIND: &'static str = "signal";
//! }
//! ```
//!
//! `#[field_trait]` puts a hidden macro beside the trait, under the trait's name, so
//! importing the trait also imports what `#[field_impl]` needs. Types named in the trait's
//! method signatures resolve where `#[field_impl]` is used.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use std::hash::{DefaultHasher, Hash, Hasher};
use syn::punctuated::Punctuated;
use syn::{
    Data, DeriveInput, Error, Fields, FnArg, Ident, ImplItem, Item, ItemImpl, ItemTrait, Path,
    ReceiverKind, ReturnType, Safety, Signature, Token, TraitItem, Type, parse_macro_input,
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
    let mut required_non_getters = Vec::new();
    for trait_item in &item.items {
        match trait_item {
            TraitItem::Fn(method) if method.default.is_none() => match getter(&method.sig) {
                Ok(getter) => getters.push(getter),
                Err(error) => errors.push(error),
            },
            TraitItem::Const(constant) if constant.default.is_none() => {
                required_non_getters.push(&constant.ident)
            }
            TraitItem::Type(ty) if ty.default.is_none() => required_non_getters.push(&ty.ident),
            TraitItem::Fn(_) | TraitItem::Const(_) | TraitItem::Type(_) => {}
            other => errors.push(Error::new_spanned(
                other,
                "#[field_trait] does not support this trait item",
            )),
        }
    }

    let errors = errors.into_iter().map(Error::into_compile_error);
    let companion = companion_macro(&item, &getters, &required_non_getters);
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

impl Getter {
    fn read_field(&self, value: TokenStream2) -> TokenStream2 {
        let field = &self.sig.ident;
        if self.by_ref {
            quote!(&#value.#field)
        } else {
            quote!(#value.#field.clone())
        }
    }
}

fn getter(sig: &Signature) -> Result<Getter, Error> {
    if sig.constness.is_some()
        || sig.asyncness.is_some()
        || !matches!(sig.safety, Safety::Default)
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
                if receiver.mutability.is_none()
                    && matches!(receiver.kind, ReceiverKind::Reference(_, _, None))
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

fn companion_macro(
    item: &ItemTrait,
    getters: &[Getter],
    required_non_getters: &[&Ident],
) -> TokenStream2 {
    let mut hasher = DefaultHasher::new();
    item.to_token_stream().to_string().hash(&mut hasher);
    let trait_name = &item.ident;
    let macro_name = format_ident!("__field_trait_{}_{:016x}", trait_name, hasher.finish());
    let vis = &item.vis;

    let struct_methods: Vec<_> = getters
        .iter()
        .map(|getter| {
            let sig = &getter.sig;
            let body = getter.read_field(quote!(self));
            quote!(#sig { #body })
        })
        .collect();
    let enum_methods = getters.iter().map(|getter| {
        let sig = &getter.sig;
        let body = getter.read_field(quote!(inner));
        quote! {
            #sig {
                match self {
                    $(Self::$variant(inner) => #body,)*
                }
            }
        }
    });
    let getter_arms = getters.iter().zip(&struct_methods).map(|(getter, method)| {
        let name = &getter.sig.ident;
        quote!((getter #name { $($not_a_getter:tt)* }) => { #method };)
    });

    let impl_arms = if required_non_getters.is_empty() {
        quote! {
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
    } else {
        let names = required_non_getters
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let message = format!(
            "#[field_impl] cannot implement `{trait_name}` because {names} must be written by hand; \
             put #[field_impl] on a hand-written impl block instead"
        );
        quote! {
            ($($input:tt)*) => { ::core::compile_error!(#message); };
        }
    };

    quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_name {
            #(#getter_arms)*
            (getter $name:ident { $($not_a_getter:tt)* }) => { $($not_a_getter)* };
            #impl_arms
        }
        #[doc(hidden)]
        #[allow(unused_imports)]
        #vis use #macro_name as #trait_name;
    }
}

#[proc_macro_attribute]
pub fn field_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args with Punctuated::<Path, Token![,]>::parse_terminated);
    let original = TokenStream2::from(input.clone());
    let output = match parse_macro_input!(input as Item) {
        Item::Impl(item) => impl_getters(&args, item),
        Item::Struct(_) | Item::Enum(_) | Item::Union(_) => syn::parse2(original.clone())
            .and_then(|item| impl_traits(&args, &item))
            .map(|impls| quote!(#original #impls)),
        _ => Err(Error::new(
            Span::call_site(),
            "#[field_impl] supports structs, enums and trait impl blocks",
        )),
    };
    match output {
        Ok(output) => output.into(),
        Err(error) => {
            let error = error.into_compile_error();
            quote!(#original #error).into()
        }
    }
}

fn impl_getters(
    args: &Punctuated<Path, Token![,]>,
    mut item: ItemImpl,
) -> Result<TokenStream2, Error> {
    let Some((trait_path, _)) = &item.trait_ else {
        return Err(Error::new_spanned(
            item.impl_token,
            "#[field_impl] on an impl block needs a trait impl",
        ));
    };
    if args.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "#[field_impl] on an impl block needs at least one getter name",
        ));
    }
    let trait_name = &trait_path.segments.last().unwrap().ident;
    let mut getters = Vec::new();
    for arg in args {
        let name = arg.get_ident().ok_or_else(|| {
            Error::new_spanned(arg, "#[field_impl] on an impl block takes getter names")
        })?;
        let message = format!("`{name}` is not a getter of `{trait_name}`");
        let not_a_getter = quote_spanned!(name.span()=> ::core::compile_error!(#message););
        getters.push(ImplItem::Verbatim(
            quote!(#trait_path! { getter #name { #not_a_getter } }),
        ));
    }
    item.items.extend(getters);
    Ok(item.into_token_stream())
}

fn impl_traits(
    trait_paths: &Punctuated<Path, Token![,]>,
    item: &DeriveInput,
) -> Result<TokenStream2, Error> {
    let shape = match &item.data {
        Data::Struct(_) => quote!(struct),
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
            if let Some(mut first) = errors.next() {
                errors.for_each(|error| first.combine(error));
                return Err(first);
            }
            let variants = data.variants.iter().map(|variant| &variant.ident);
            quote!(enum [#(#variants)*])
        }
        Data::Union(data) => {
            return Err(Error::new_spanned(
                data.union_token,
                "#[field_impl] supports structs, enums and trait impl blocks",
            ));
        }
    };
    if trait_paths.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "#[field_impl] needs at least one trait",
        ));
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
    Ok(quote!(#(#impls)*))
}
