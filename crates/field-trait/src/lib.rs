//! Implement getter-style traits from same-named fields.
//!
//! ```ignore
//! use field_trait::{field_trait, field_trait_impl};
//!
//! #[field_trait]
//! pub trait HasWeight {
//!     fn weight(&self) -> u32;
//!     fn name(&self) -> &str;
//! }
//!
//! #[field_trait_impl(HasWeight)]
//! struct Widget { weight: u32, name: String }
//!
//! #[field_trait_impl(HasWeight)]
//! enum Item { Widget(Widget), Gadget(Gadget) }
//! ```
//!
//! `fn x(&self) -> T` returns `self.x.clone()` (so `T` must be `Clone`) and `fn x(&self) -> &T`
//! returns `&self.x`. Methods with default bodies are left alone. On an enum, every
//! variant must hold exactly one unnamed field, and each method reads the field from that
//! inner value.
//!
//! On a struct or enum, `#[field_trait_impl]` takes trait names and writes each whole impl. On
//! a hand-written `impl Trait for Type` block, it takes no arguments and adds every getter
//! the block does not define, which lets a trait with associated consts or types that have
//! no default use it. The impl block form supports structs only, because it cannot see the
//! type definition:
//!
//! ```ignore
//! #[field_trait_impl]
//! impl HasKind for Part {
//!     const KIND: &'static str = "part";
//! }
//! ```
//!
//! `#[with_trait_fields(Trait, ...)]` on a struct with named fields appends a field for
//! each getter of the listed traits that the struct does not already declare. The field
//! has the getter's return type with any reference removed, and the trait's visibility. It
//! must be the first attribute, so the attributes below it see the added fields. This is
//! not inheritance: struct literals still set every field, and the added fields do not
//! appear in the struct's source.
//!
//! ```ignore
//! #[with_trait_fields(HasWeight)]
//! #[field_trait_impl(HasWeight)]
//! #[derive(Deserialize)]
//! struct Gizmo { name: String }
//! ```
//!
//! `#[field_trait]` puts a hidden macro beside the trait, under the trait's name, so
//! importing the trait also imports what the other attributes need. Types named in the
//! trait's method signatures resolve where those attributes are used.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, format_ident, quote};
use std::hash::{DefaultHasher, Hash, Hasher};
use syn::punctuated::Punctuated;
use syn::{
    Data, DeriveInput, Error, Fields, FnArg, Ident, ImplItem, Item, ItemImpl, ItemStruct,
    ItemTrait, Path, ReceiverKind, ReturnType, Safety, Signature, Token, TraitItem, Type,
    parse_macro_input,
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

    fn field_type(&self) -> Result<&Type, Error> {
        let ReturnType::Type(_, returned) = &self.sig.output else {
            unreachable!("getters always return a type");
        };
        let ty = match &**returned {
            Type::Reference(reference) => &*reference.elem,
            ty => ty,
        };
        match ty {
            Type::Slice(_) | Type::TraitObject(_) => {}
            Type::Path(path) if path.qself.is_none() && path.path.is_ident("str") => {}
            ty => return Ok(ty),
        }
        Err(Error::new_spanned(
            returned,
            "#[with_trait_fields] cannot declare a field for an unsized return type",
        ))
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
    let getter_names: Vec<_> = getters.iter().map(|getter| &getter.sig.ident).collect();
    let fill_arms = quote! {
        (fill [$($trait_path:tt)*] [$($defined:ident)*]) => {
            #($($trait_path)*! { @fill #getter_names [$($trait_path)*] [$($defined)*] })*
        };
        #((@fill #getter_names [$($trait_path:tt)*] [#getter_names $($rest:ident)*]) => {};)*
        #((@fill #getter_names [$($trait_path:tt)*] []) => { #struct_methods };)*
        (@fill $name:ident [$($trait_path:tt)*] [$first:ident $($rest:ident)*]) => {
            $($trait_path)*! { @fill $name [$($trait_path)*] [$($rest)*] }
        };
    };
    let add_field_arms = getters.iter().map(|getter| {
        let name = &getter.sig.ident;
        let (field, error) = match getter.field_type() {
            Ok(ty) => (quote!(#vis #name: #ty,), None),
            Err(error) => (quote!(), Some(error.into_compile_error())),
        };
        quote! {
            (
                @field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*]
                [#name $($pending:ident)*] [] $($next:tt)*
            ) => {
                #error
                $($me)*! {
                    @next_field [$($me)*] [$($prefix)*] [$($body)* #field] [$($defined)* #name]
                    [$($pending)*] $($next)*
                }
            };
        }
    });
    let field_arms = quote! {
        (@fields [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*] $($next:tt)*) => {
            $($me)*! {
                @next_field [$($me)*] [$($prefix)*] [$($body)*] [$($defined)*]
                [#(#getter_names)*] $($next)*
            }
        };
        #((
            @field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*]
            [#getter_names $($pending:ident)*] [#getter_names $($searched:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @next_field [$($me)*] [$($prefix)*] [$($body)*] [$($defined)*]
                [$($pending)*] $($next)*
            }
        };)*
        #(#add_field_arms)*
        (
            @field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*]
            [$name:ident $($pending:ident)*] [$first:ident $($searched:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @field [$($me)*] [$($prefix)*] [$($body)*] [$($defined)*]
                [$name $($pending)*] [$($searched)*] $($next)*
            }
        };
        (
            @next_field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*]
            [$name:ident $($pending:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @field [$($me)*] [$($prefix)*] [$($body)*] [$($defined)*]
                [$name $($pending)*] [$($defined)*] $($next)*
            }
        };
        (@next_field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*] []) => {
            $($prefix)* { $($body)* }
        };
        (
            @next_field [$($me:tt)*] [$($prefix:tt)*] [$($body:tt)*] [$($defined:ident)*] []
            [$($next_trait:tt)*] $($rest:tt)*
        ) => {
            $($next_trait)*! {
                @fields [$($next_trait)*] [$($prefix)*] [$($body)*] [$($defined)*] $($rest)*
            }
        };
    };

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
            "#[field_trait_impl] cannot implement `{trait_name}` because {names} must be written by hand; \
             put #[field_trait_impl] on a hand-written impl block instead"
        );
        quote! {
            ($($input:tt)*) => { ::core::compile_error!(#message); };
        }
    };

    quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_name {
            #fill_arms
            #field_arms
            #impl_arms
        }
        #[doc(hidden)]
        #[allow(unused_imports)]
        #vis use #macro_name as #trait_name;
    }
}

#[proc_macro_attribute]
pub fn field_trait_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args with Punctuated::<Path, Token![,]>::parse_terminated);
    let original = TokenStream2::from(input.clone());
    let output = match parse_macro_input!(input as Item) {
        Item::Impl(item) => impl_getters(&args, item),
        Item::Struct(_) | Item::Enum(_) | Item::Union(_) => syn::parse2(original.clone())
            .and_then(|item| impl_traits(&args, &item))
            .map(|impls| quote!(#original #impls)),
        _ => Err(Error::new(
            Span::call_site(),
            "#[field_trait_impl] supports structs, enums and trait impl blocks",
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
            "#[field_trait_impl] on an impl block needs a trait impl",
        ));
    };
    if !args.is_empty() {
        return Err(Error::new_spanned(
            args,
            "#[field_trait_impl] on an impl block takes no arguments",
        ));
    }
    let defined = item.items.iter().filter_map(|impl_item| match impl_item {
        ImplItem::Fn(method) => Some(&method.sig.ident),
        _ => None,
    });
    let fill = quote!(#trait_path! { fill [#trait_path] [#(#defined)*] });
    item.items.push(ImplItem::Verbatim(fill));
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
                        "#[field_trait_impl] enum variants must hold exactly one unnamed field",
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
                "#[field_trait_impl] supports structs, enums and trait impl blocks",
            ));
        }
    };
    if trait_paths.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "#[field_trait_impl] needs at least one trait",
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

#[proc_macro_attribute]
pub fn with_trait_fields(args: TokenStream, input: TokenStream) -> TokenStream {
    let trait_paths = parse_macro_input!(args with Punctuated::<Path, Token![,]>::parse_terminated);
    let original = TokenStream2::from(input.clone());
    let item = parse_macro_input!(input as ItemStruct);
    let Fields::Named(fields) = &item.fields else {
        let error = Error::new_spanned(&item.fields, "#[with_trait_fields] needs named fields")
            .into_compile_error();
        return quote!(#original #error).into();
    };
    let mut trait_paths = trait_paths.iter();
    let Some(first_trait) = trait_paths.next() else {
        let error = Error::new(
            Span::call_site(),
            "#[with_trait_fields] needs at least one trait",
        )
        .into_compile_error();
        return quote!(#original #error).into();
    };

    let ItemStruct {
        attrs,
        vis,
        struct_token,
        ident,
        generics,
        ..
    } = &item;
    let where_clause = &generics.where_clause;
    let fields = fields.named.iter();
    let defined = fields.clone().filter_map(|field| field.ident.as_ref());
    quote! {
        #first_trait! {
            @fields [#first_trait]
            [#(#attrs)* #vis #struct_token #ident #generics #where_clause]
            [#(#fields,)*] [#(#defined)*]
            #([#trait_paths])*
        }
    }
    .into()
}
