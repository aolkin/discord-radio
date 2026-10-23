//! Implement getter-style traits from fields.
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
//! a hand-written `impl Trait for Type` block, it takes no trait names and adds every getter
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
//! A `getter = field` pair makes a getter read `field` instead of the field named after the
//! getter. Each pair applies to the nearest trait listed before it. On an impl block, the
//! pairs are the only arguments and apply to the implemented trait.
//!
//! ```ignore
//! #[field_trait_impl(HasWeight, weight = mass, HasScale)]
//! struct Sprocket { mass: u32, name: String, scale: f32 }
//!
//! #[field_trait_impl(rank = level)]
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
//! appear in the struct's source. It takes the same `getter = field` pairs, and then
//! checks for and adds the mapped field instead.
//!
//! ```ignore
//! #[with_trait_fields(HasWeight, weight = mass)]
//! #[field_trait_impl(HasWeight, weight = mass)]
//! #[derive(Deserialize)]
//! struct Gizmo { name: String }
//! ```
//!
//! A trait may have type parameters, with or without bounds and defaults, but no lifetime or
//! const parameters and no where clause. Name the trait with its type arguments, and the
//! generated impl adds the trait's bounds on its parameters to its where clause:
//!
//! ```ignore
//! #[field_trait]
//! pub trait HasParts<T: Clone> {
//!     fn parts(&self) -> &Vec<T>;
//! }
//!
//! #[field_trait_impl(HasParts<Gadget>)]
//! struct Toolbox { parts: Vec<Gadget> }
//! ```
//!
//! A trait implemented by `Widget` is not implemented by `Arc<Widget>`, so a generic bound
//! like `T: HasWeight` rejects it even though method calls on it auto-deref.
//! `#[field_trait(Arc, Rc, Box)]` takes a list of wrapper types and implements the trait for
//! each wrapper around any type that implements it, forwarding every getter to the inner
//! value and every associated type and const to the inner type's. A wrapper must take one
//! type parameter and deref to it:
//!
//! ```ignore
//! #[field_trait(Arc, Box)]
//! pub trait HasWeight {
//!     fn weight(&self) -> u32;
//! }
//!
//! fn heaviest<T: HasWeight>(items: &[T]) -> Option<u32> { ... }
//!
//! heaviest(&[Arc::new(widget)]);
//! ```
//!
//! `#[field_trait(Deref)]` instead implements the trait once for every type that derefs to a
//! type implementing it, so it cannot be combined with wrapper names. Under that impl, a
//! type that implements both `Deref` and the trait directly is a conflicting impl, and so
//! is an impl for a type from another crate, because that crate could add a `Deref` impl
//! later.
//!
//! `#[field_trait]` puts a hidden macro beside the trait, under the trait's name, so
//! importing the trait also imports what the other attributes need. Types named in the
//! trait's method signatures resolve where those attributes are used.

use proc_macro::TokenStream;
use proc_macro2::{Group, Span, TokenStream as TokenStream2, TokenTree};
use quote::{ToTokens, format_ident, quote};
use std::hash::{DefaultHasher, Hash, Hasher};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    Data, DeriveInput, Error, Fields, FnArg, GenericArgument, GenericParam, Ident, ImplItem, Item,
    ItemImpl, ItemStruct, ItemTrait, Path, PathArguments, ReceiverKind, ReturnType, Safety,
    Signature, Token, TraitItem, Type, TypeParam, TypeParamBound, parse_macro_input,
};

#[proc_macro_attribute]
pub fn field_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    let wrappers = parse_macro_input!(args with Punctuated::<Path, Token![,]>::parse_terminated);
    let item = parse_macro_input!(input as ItemTrait);
    let mut errors = Vec::new();
    for wrapper in &wrappers {
        if let Some(segment) = wrapper
            .segments
            .iter()
            .find(|segment| !segment.arguments.is_none())
        {
            errors.push(Error::new_spanned(
                segment,
                "#[field_trait] wrappers must be named without type arguments, like `Arc`",
            ));
        }
    }
    if wrappers.len() > 1
        && let Some(deref) = wrappers.iter().find(|wrapper| wrapper.is_ident("Deref"))
    {
        errors.push(Error::new_spanned(
            deref,
            "#[field_trait] cannot combine `Deref` with other wrappers, \
             because its impl already covers every type that derefs",
        ));
    }
    for param in &item.generics.params {
        if !matches!(param, GenericParam::Type(_)) {
            errors.push(Error::new_spanned(
                param,
                "#[field_trait] supports only type parameters on the trait",
            ));
        }
    }
    if let Some(where_clause) = &item.generics.where_clause {
        errors.push(Error::new_spanned(
            where_clause,
            "#[field_trait] does not support where clauses on the trait",
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

    let wrapper_impls = if errors.is_empty() {
        wrappers
            .iter()
            .map(|wrapper| forwarding_impl(&item, &getters, wrapper))
            .collect()
    } else {
        TokenStream2::new()
    };
    let errors = errors.into_iter().map(Error::into_compile_error);
    let companion = companion_macro(&item, &getters, &required_non_getters);
    quote! {
        #item
        #companion
        #wrapper_impls
        #(#errors)*
    }
    .into()
}

fn forwarding_impl(item: &ItemTrait, getters: &[Getter], wrapper: &Path) -> TokenStream2 {
    let trait_name = &item.ident;
    let unsafety = &item.unsafety;
    let mut generics = item.generics.clone();
    for param in generics.type_params_mut() {
        param.default = None;
    }
    let (_, ty_generics, _) = item.generics.split_for_impl();
    let params = &generics.params;
    let trait_ty = quote!(#trait_name #ty_generics);
    let (header, target) = if wrapper.is_ident("Deref") {
        let deref = Ident::new("__FieldTraitDeref", Span::call_site());
        let target = quote!(<#deref as ::core::ops::Deref>::Target);
        let header = quote! {
            impl<#deref: ::core::ops::Deref + ?::core::marker::Sized, #params> #trait_ty for #deref
            where #target: #trait_ty
        };
        (header, target)
    } else {
        let inner = Ident::new("__FieldTraitInner", Span::call_site());
        let header = quote! {
            impl<#inner: #trait_ty + ?::core::marker::Sized, #params> #trait_ty for #wrapper<#inner>
        };
        (header, quote!(#inner))
    };
    let associated = item.items.iter().filter_map(|trait_item| match trait_item {
        TraitItem::Const(constant) => {
            let name = &constant.ident;
            let ty = &constant.ty;
            Some(quote!(const #name: #ty = <#target as #trait_ty>::#name;))
        }
        TraitItem::Type(ty) => {
            let name = &ty.ident;
            Some(quote!(type #name = <#target as #trait_ty>::#name;))
        }
        _ => None,
    });
    let methods = getters.iter().map(|getter| {
        let sig = &getter.sig;
        let name = &sig.ident;
        quote!(#sig { <#target as #trait_ty>::#name(&**self) })
    });
    quote! {
        #unsafety #header {
            #(#associated)*
            #(#methods)*
        }
    }
}

struct Getter {
    sig: Signature,
    by_ref: bool,
}

impl Getter {
    fn read(&self, shape: TokenStream2) -> TokenStream2 {
        let name = &self.sig.ident;
        let mode = if self.by_ref {
            quote!(by_ref)
        } else {
            quote!(cloned)
        };
        quote!($($me)*!(@read #name [$($me)*] [$($map)*] #mode #shape))
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

fn metavar(param: &Ident) -> TokenStream2 {
    let name = format_ident!("__field_trait_{}", param);
    quote!($#name)
}

fn substitute(
    tokens: TokenStream2,
    replace: &dyn Fn(&Ident) -> Option<TokenStream2>,
) -> TokenStream2 {
    tokens
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Ident(ident) => {
                replace(&ident).unwrap_or_else(|| TokenTree::Ident(ident).into())
            }
            TokenTree::Group(group) => {
                let mut substituted =
                    Group::new(group.delimiter(), substitute(group.stream(), replace));
                substituted.set_span(group.span());
                TokenTree::Group(substituted).into()
            }
            other => other.into(),
        })
        .collect()
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

    let params: Vec<&TypeParam> = item.generics.type_params().collect();
    let metavars: Vec<_> = params.iter().map(|param| metavar(&param.ident)).collect();
    let to_metavar = |ident: &Ident| {
        params
            .iter()
            .find(|param| param.ident == *ident)
            .map(|param| metavar(&param.ident))
    };
    let params_pattern = quote!(#(#metavars:ty),*);
    let trait_args = if params.is_empty() {
        quote!()
    } else {
        quote!(<#(#metavars),*>)
    };
    let param_bounds: Vec<_> = params
        .iter()
        .zip(&metavars)
        .filter_map(|(param, metavar)| {
            let bounds: Vec<_> = param
                .bounds
                .iter()
                .filter(|bound| !matches!(bound, TypeParamBound::Trait(t) if t.maybe.is_some()))
                .collect();
            if bounds.is_empty() {
                return None;
            }
            let bounds = substitute(quote!(#(#bounds)+*), &to_metavar);
            Some(quote!(#metavar: #bounds,))
        })
        .collect();

    let required = params
        .iter()
        .take_while(|param| param.default.is_none())
        .count();
    let normalize_arms = (required..=params.len()).map(|supplied| {
        let mut args: Vec<TokenStream2> = Vec::new();
        for (index, param) in params.iter().enumerate() {
            let arg = if index < supplied {
                metavar(&param.ident)
            } else if let Some((_, default)) = &param.default {
                substitute(default.to_token_stream(), &|ident| {
                    params[..index]
                        .iter()
                        .position(|earlier| earlier.ident == *ident)
                        .map(|position| args[position].clone())
                })
            } else {
                return quote!();
            };
            args.push(arg);
        }
        let pattern = metavars[..supplied].iter();
        quote! {
            ([$($me:tt)*] [#(#pattern:ty),*] $kind:tt $($rest:tt)*) => {
                $($me)*! { @ $kind [$($me)*] [#(#args),*] $($rest)* }
            };
        }
    });
    let wrong_args = format!("wrong number of type arguments for `{trait_name}`");

    let struct_methods: Vec<_> = getters
        .iter()
        .map(|getter| {
            let sig = substitute(getter.sig.to_token_stream(), &to_metavar);
            let body = getter.read(quote!(struct self));
            quote!(#sig { #body })
        })
        .collect();
    let enum_methods = getters.iter().map(|getter| {
        let sig = substitute(getter.sig.to_token_stream(), &to_metavar);
        let body = getter.read(quote!(enum self [$($variant)*]));
        quote!(#sig { #body })
    });
    let getter_names: Vec<_> = getters.iter().map(|getter| &getter.sig.ident).collect();
    let not_a_getter = format!("` is not a getter of `{trait_name}`");
    let map_arms = quote! {
        #((@read #getter_names [$($me:tt)*] [(#getter_names $field:ident) $($map:tt)*] $($rest:tt)*) => {
            $($me)*!(@read_field $field $($rest)*)
        };)*
        (@read $name:ident [$($me:tt)*] [$entry:tt $($map:tt)*] $($rest:tt)*) => {
            $($me)*!(@read $name [$($me)*] [$($map)*] $($rest)*)
        };
        (@read $name:ident [$($me:tt)*] [] $($rest:tt)*) => {
            $($me)*!(@read_field $name $($rest)*)
        };
        (@read_field $field:ident by_ref struct $value:tt) => { &$value.$field };
        (@read_field $field:ident cloned struct $value:tt) => { $value.$field.clone() };
        (@read_field $field:ident by_ref enum $value:tt [$($variant:ident)*]) => {
            match $value { $(Self::$variant(inner) => &inner.$field,)* }
        };
        (@read_field $field:ident cloned enum $value:tt [$($variant:ident)*]) => {
            match $value { $(Self::$variant(inner) => inner.$field.clone(),)* }
        };
        #((@check [$($me:tt)*] [(#getter_names $($field:ident)?) $($map:tt)*]) => {
            $($me)*! { @check [$($me)*] [$($map)*] }
        };)*
        (@check [$($me:tt)*] []) => {};
        (@check [$($me:tt)*] [($name:ident $($field:ident)?) $($map:tt)*]) => {
            ::core::compile_error!(::core::concat!("`", ::core::stringify!($name), #not_a_getter));
        };
    };
    let fill_arms = quote! {
        (@fill [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($defined:ident)*]) => {
            $($me)*! { @check [$($me)*] [$($map)*] }
            #($($me)*! { @fill_getter #getter_names [$($me)*] [$($args)*] [$($map)*] [$($defined)*] })*
        };
        #((
            @fill_getter #getter_names [$($me:tt)*] [$($args:tt)*] [$($map:tt)*]
            [#getter_names $($rest:ident)*]
        ) => {};)*
        #((@fill_getter #getter_names [$($me:tt)*] [#params_pattern] [$($map:tt)*] []) => {
            #struct_methods
        };)*
        (
            @fill_getter $name:ident [$($me:tt)*] [$($args:tt)*] [$($map:tt)*]
            [$first:ident $($rest:ident)*]
        ) => {
            $($me)*! { @fill_getter $name [$($me)*] [$($args)*] [$($map)*] [$($rest)*] }
        };
    };
    let add_field_arms = getters.iter().map(|getter| {
        let name = &getter.sig.ident;
        let (ty, error) = match getter.field_type() {
            Ok(ty) => (Some(substitute(ty.to_token_stream(), &to_metavar)), None),
            Err(error) => (None, Some(error.into_compile_error())),
        };
        let field = |field: TokenStream2| ty.as_ref().map(|ty| quote!(#vis #field: #ty,));
        let named_field = field(quote!(#name));
        let mapped_field = field(quote!($field));
        quote! {
            (
                @mapped [$($me:tt)*] [#params_pattern] [$($map:tt)*] [$($prefix:tt)*]
                [$($body:tt)*] [$($defined:ident)*] [#name $($pending:ident)*]
                [(#name $field:ident) $($searched:tt)*] $($next:tt)*
            ) => {
                #error
                $($me)*! {
                    @next_field [$($me)*] [#(#metavars),*] [$($map)*] [$($prefix)*]
                    [$($body)* #mapped_field] [$($defined)* $field] [$($pending)*] $($next)*
                }
            };
            (
                @field [$($me:tt)*] [#params_pattern] [$($map:tt)*] [$($prefix:tt)*]
                [$($body:tt)*] [$($defined:ident)*] [#name $($pending:ident)*] [] $($next:tt)*
            ) => {
                #error
                $($me)*! {
                    @next_field [$($me)*] [#(#metavars),*] [$($map)*] [$($prefix)*]
                    [$($body)* #named_field] [$($defined)* #name] [$($pending)*] $($next)*
                }
            };
        }
    });
    let field_arms = quote! {
        (
            @fields [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] $($next:tt)*
        ) => {
            $($me)*! { @check [$($me)*] [$($map)*] }
            $($me)*! {
                @next_field [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [#(#getter_names)*] $($next)*
            }
        };
        #(#add_field_arms)*
        #((
            @mapped [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [#getter_names $($pending:ident)*]
            [(#getter_names) $($searched:tt)*] $($next:tt)*
        ) => {
            $($me)*! {
                @next_field [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$($pending)*] $($next)*
            }
        };)*
        (
            @mapped [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [$name:ident $($pending:ident)*]
            [$entry:tt $($searched:tt)*] $($next:tt)*
        ) => {
            $($me)*! {
                @mapped [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$name $($pending)*] [$($searched)*] $($next)*
            }
        };
        (
            @mapped [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [$name:ident $($pending:ident)*] [] $($next:tt)*
        ) => {
            $($me)*! {
                @field [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$name $($pending)*] [$($defined)*] $($next)*
            }
        };
        #((
            @field [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [#getter_names $($pending:ident)*]
            [#getter_names $($searched:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @next_field [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$($pending)*] $($next)*
            }
        };)*
        (
            @field [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [$name:ident $($pending:ident)*]
            [$first:ident $($searched:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @field [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$name $($pending)*] [$($searched)*] $($next)*
            }
        };
        (
            @next_field [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [$name:ident $($pending:ident)*] $($next:tt)*
        ) => {
            $($me)*! {
                @mapped [$($me)*] [$($args)*] [$($map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] [$name $($pending)*] [$($map)*] $($next)*
            }
        };
        (
            @next_field [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] []
        ) => {
            $($prefix)* { $($body)* }
        };
        (
            @next_field [$($me:tt)*] [$($args:tt)*] [$($map:tt)*] [$($prefix:tt)*] [$($body:tt)*]
            [$($defined:ident)*] [] [$($next_me:tt)*] [$($next_args:tt)*] [$($next_map:tt)*]
            $($rest:tt)*
        ) => {
            $($next_me)*! {
                [$($next_me)*] [$($next_args)*] fields [$($next_map)*] [$($prefix)*] [$($body)*]
                [$($defined)*] $($rest)*
            }
        };
    };

    let impl_arms = if required_non_getters.is_empty() {
        quote! {
            (
                @impl [$($me:tt)*] [#params_pattern] [$($map:tt)*] [$($impl_generics:tt)*]
                [$($self_ty:tt)*] [$($predicates:tt)*] struct
            ) => {
                $($me)*! { @check [$($me)*] [$($map)*] }
                impl $($impl_generics)* $($me)* #trait_args for $($self_ty)*
                where $($predicates)* #(#param_bounds)*
                {
                    #(#struct_methods)*
                }
            };
            (
                @impl [$($me:tt)*] [#params_pattern] [$($map:tt)*] [$($impl_generics:tt)*]
                [$($self_ty:tt)*] [$($predicates:tt)*] enum [$($variant:ident)*]
            ) => {
                $($me)*! { @check [$($me)*] [$($map)*] }
                impl $($impl_generics)* $($me)* #trait_args for $($self_ty)*
                where $($predicates)* #(#param_bounds)*
                {
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
            (@impl $($input:tt)*) => { ::core::compile_error!(#message); };
        }
    };

    quote! {
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #macro_name {
            #map_arms
            #fill_arms
            #field_arms
            #impl_arms
            #(#normalize_arms)*
            ([$($me:tt)*] $($rest:tt)*) => { ::core::compile_error!(#wrong_args); };
        }
        #[doc(hidden)]
        #[allow(unused_imports)]
        #vis use #macro_name as #trait_name;
    }
}

struct FieldMapping {
    getter: Ident,
    field: Ident,
}

enum Arg {
    Trait(Path),
    Field(FieldMapping),
}

impl Parse for Arg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(Ident) && input.peek2(Token![=]) {
            let getter = input.parse()?;
            input.parse::<Token![=]>()?;
            let field = input.parse()?;
            Ok(Arg::Field(FieldMapping { getter, field }))
        } else {
            input.parse().map(Arg::Trait)
        }
    }
}

type Args = Punctuated<Arg, Token![,]>;

struct TraitRef {
    macro_path: Path,
    args: Vec<Type>,
    fields: Vec<FieldMapping>,
}

impl TraitRef {
    fn map_field(&mut self, mapping: &FieldMapping) -> Result<(), Error> {
        if self
            .fields
            .iter()
            .any(|existing| existing.getter == mapping.getter)
        {
            return Err(Error::new_spanned(
                &mapping.getter,
                format!("`{}` is mapped to a field more than once", mapping.getter),
            ));
        }
        self.fields.push(FieldMapping {
            getter: mapping.getter.clone(),
            field: mapping.field.clone(),
        });
        Ok(())
    }

    fn read_map(&self) -> TokenStream2 {
        let entries = self
            .fields
            .iter()
            .map(|FieldMapping { getter, field }| quote!((#getter #field)));
        quote!([#(#entries)*])
    }

    fn parse(path: &Path) -> Result<Self, Error> {
        let mut macro_path = path.clone();
        let last = macro_path
            .segments
            .last_mut()
            .expect("a parsed path has at least one segment");
        let arguments = std::mem::replace(&mut last.arguments, PathArguments::None);
        let args = match arguments {
            PathArguments::None => Vec::new(),
            PathArguments::AngleBracketed(arguments) => arguments
                .args
                .into_iter()
                .map(|argument| match argument {
                    GenericArgument::Type(ty) => Ok(ty),
                    other => Err(Error::new_spanned(
                        other,
                        "#[field_trait] traits take only type arguments",
                    )),
                })
                .collect::<Result<_, _>>()?,
            arguments @ PathArguments::Parenthesized(_) => {
                return Err(Error::new_spanned(
                    arguments,
                    "#[field_trait] traits take only type arguments",
                ));
            }
        };
        Ok(TraitRef {
            macro_path,
            args,
            fields: Vec::new(),
        })
    }

    fn invoke(&self, kind: TokenStream2, map: TokenStream2, rest: TokenStream2) -> TokenStream2 {
        let TraitRef {
            macro_path, args, ..
        } = self;
        quote!(#macro_path! { [#macro_path] [#(#args),*] #kind #map #rest })
    }

    fn bracketed(&self, map: TokenStream2) -> TokenStream2 {
        let TraitRef {
            macro_path, args, ..
        } = self;
        quote!([#macro_path] [#(#args),*] #map)
    }
}

fn trait_refs(args: &Args) -> Result<Vec<TraitRef>, Error> {
    let mut trait_refs: Vec<TraitRef> = Vec::new();
    for arg in args {
        match arg {
            Arg::Trait(path) => trait_refs.push(TraitRef::parse(path)?),
            Arg::Field(mapping) => match trait_refs.last_mut() {
                Some(trait_ref) => trait_ref.map_field(mapping)?,
                None => {
                    return Err(Error::new_spanned(
                        &mapping.getter,
                        "`getter = field` must follow the trait it applies to",
                    ));
                }
            },
        }
    }
    Ok(trait_refs)
}

#[proc_macro_attribute]
pub fn field_trait_impl(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args with Args::parse_terminated);
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

fn impl_getters(args: &Args, mut item: ItemImpl) -> Result<TokenStream2, Error> {
    let Some((trait_path, _)) = &item.trait_ else {
        return Err(Error::new_spanned(
            item.impl_token,
            "#[field_trait_impl] on an impl block needs a trait impl",
        ));
    };
    let mut trait_ref = TraitRef::parse(trait_path)?;
    for arg in args {
        match arg {
            Arg::Field(mapping) => trait_ref.map_field(mapping)?,
            Arg::Trait(path) => {
                return Err(Error::new_spanned(
                    path,
                    "#[field_trait_impl] on an impl block takes only `getter = field` pairs",
                ));
            }
        }
    }
    let defined = item.items.iter().filter_map(|impl_item| match impl_item {
        ImplItem::Fn(method) => Some(&method.sig.ident),
        _ => None,
    });
    let fill = trait_ref.invoke(quote!(fill), trait_ref.read_map(), quote!([#(#defined)*]));
    item.items.push(ImplItem::Verbatim(fill));
    Ok(item.into_token_stream())
}

fn impl_traits(args: &Args, item: &DeriveInput) -> Result<TokenStream2, Error> {
    let trait_refs = trait_refs(args)?;
    let shape = match &item.data {
        Data::Struct(data) => {
            let mut errors = trait_refs
                .iter()
                .flat_map(|trait_ref| &trait_ref.fields)
                .filter(|mapping| {
                    !data
                        .fields
                        .iter()
                        .any(|field| field.ident.as_ref() == Some(&mapping.field))
                })
                .map(|mapping| {
                    Error::new_spanned(
                        &mapping.field,
                        format!("`{}` has no field `{}`", item.ident, mapping.field),
                    )
                });
            if let Some(mut first) = errors.next() {
                errors.for_each(|error| first.combine(error));
                return Err(first);
            }
            quote!(struct)
        }
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
    if trait_refs.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "#[field_trait_impl] needs at least one trait",
        ));
    }

    let name = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();
    let predicates = where_clause.map(|where_clause| where_clause.predicates.iter());
    let predicates = predicates.into_iter().flatten();
    let rest = quote!([#impl_generics] [#name #ty_generics] [#(#predicates,)*] #shape);
    let impls = trait_refs
        .iter()
        .map(|trait_ref| trait_ref.invoke(quote!(impl), trait_ref.read_map(), rest.clone()))
        .collect::<TokenStream2>();
    Ok(impls)
}

#[proc_macro_attribute]
pub fn with_trait_fields(args: TokenStream, input: TokenStream) -> TokenStream {
    let trait_paths = parse_macro_input!(args with Args::parse_terminated);
    let original = TokenStream2::from(input.clone());
    let item = parse_macro_input!(input as ItemStruct);
    let Fields::Named(fields) = &item.fields else {
        let error = Error::new_spanned(&item.fields, "#[with_trait_fields] needs named fields")
            .into_compile_error();
        return quote!(#original #error).into();
    };
    let trait_refs = match trait_refs(&trait_paths) {
        Ok(trait_refs) => trait_refs,
        Err(error) => {
            let error = error.into_compile_error();
            return quote!(#original #error).into();
        }
    };
    let Some((first_trait, other_traits)) = trait_refs.split_first() else {
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
    let defined: Vec<&Ident> = fields
        .clone()
        .filter_map(|field| field.ident.as_ref())
        .collect();
    let mut provided = defined.clone();
    let mut maps = Vec::new();
    for trait_ref in &trait_refs {
        let mut entries = Vec::new();
        for FieldMapping { getter, field } in &trait_ref.fields {
            if provided.contains(&field) {
                entries.push(quote!((#getter)));
            } else {
                provided.push(field);
                entries.push(quote!((#getter #field)));
            }
        }
        maps.push(quote!([#(#entries)*]));
    }
    let other_traits = other_traits
        .iter()
        .zip(&maps[1..])
        .map(|(trait_ref, map)| trait_ref.bracketed(map.clone()));
    first_trait
        .invoke(
            quote!(fields),
            maps[0].clone(),
            quote! {
                [#(#attrs)* #vis #struct_token #ident #generics #where_clause]
                [#(#fields,)*] [#(#defined)*]
                #(#other_traits)*
            },
        )
        .into()
}
