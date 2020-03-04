use crate::derives::attrs::{Attrs, Kind, Name, DEFAULT_CASING, DEFAULT_ENV_CASING};
use crate::derives::{from_argmatches, into_app, spanned::Sp};

use proc_macro2::{Ident, Span, TokenStream};
use proc_macro_error::{abort, abort_call_site};
use quote::{quote, quote_spanned};
use syn::{
    punctuated::Punctuated, spanned::Spanned, Attribute, DataEnum, FieldsUnnamed, Token, Variant,
};

pub fn gen_for_enum(name: &Ident, attrs: &[Attribute], e: &DataEnum) -> TokenStream {
    let attrs = Attrs::from_struct(
        Span::call_site(),
        attrs,
        Name::Derived(name.clone()),
        Sp::call_site(DEFAULT_CASING),
        Sp::call_site(DEFAULT_ENV_CASING),
    );

    let from_subcommand = gen_from_subcommand(name, &e.variants, &attrs);
    let augment_subcommands = gen_augment_subcommands(&e.variants, &attrs);

    quote! {
        impl ::clap::Subcommand for #name {
            #augment_subcommands
            #from_subcommand
        }
    }
}

fn gen_augment_subcommands(
    variants: &Punctuated<syn::Variant, Token![,]>,
    parent_attribute: &Attrs,
) -> proc_macro2::TokenStream {
    use syn::Fields::*;

    let subcommands = variants.iter().map(|variant| {
        let attrs = Attrs::from_struct(
            variant.span(),
            &variant.attrs,
            Name::Derived(variant.ident.clone()),
            parent_attribute.casing(),
            parent_attribute.env_casing(),
        );
        let kind = attrs.kind();
        match &*kind {
            Kind::Flatten => match variant.fields {
                Unnamed(FieldsUnnamed { ref unnamed, .. }) if unnamed.len() == 1 => {
                    let ty = &unnamed[0];
                    quote! {
                        let app = <#ty as ::clap::Subcommand>::augment_subcommands(app);
                    }
                }
                _ => abort!(
                    variant,
                    "`flatten` is usable only with single-typed tuple variants"
                ),
            },

            _ => {
                let app_var = Ident::new("subcommand", Span::call_site());
                let arg_block = match variant.fields {
                    Named(ref fields) => {
                        into_app::gen_app_augmentation(&fields.named, &app_var, &attrs)
                    }
                    Unit => quote!( #app_var ),
                    Unnamed(FieldsUnnamed { ref unnamed, .. }) if unnamed.len() == 1 => {
                        let ty = &unnamed[0];
                        quote_spanned! { ty.span()=>
                            {
                                <#ty as ::clap::IntoApp>::augment_clap(#app_var)
                            }
                        }
                    }
                    Unnamed(..) => {
                        abort!(variant, "non single-typed tuple enums are not supported")
                    }
                };

                let name = attrs.cased_name();
                let from_attrs = attrs.top_level_methods();
                let version = attrs.version();
                quote! {
                    let app = app.subcommand({
                        let #app_var = ::clap::App::new(#name);
                        let #app_var = #arg_block;
                        #app_var#from_attrs#version
                    });
                }
            }
        }
    });

    let app_methods = parent_attribute.top_level_methods();
    let version = parent_attribute.version();
    quote! {
        fn augment_subcommands<'b>(app: ::clap::App<'b>) -> ::clap::App<'b> {
            let app = app #app_methods;
            #( #subcommands )*;
            app #version
        }
    }
}

fn gen_from_subcommand(
    name: &syn::Ident,
    variants: &Punctuated<Variant, Token![,]>,
    parent_attribute: &Attrs,
) -> proc_macro2::TokenStream {
    use syn::Fields::*;
    let (flatten_variants, variants): (Vec<_>, Vec<_>) = variants
        .iter()
        .map(|variant| {
            let attrs = Attrs::from_struct(
                variant.span(),
                &variant.attrs,
                Name::Derived(variant.ident.clone()),
                parent_attribute.casing(),
                parent_attribute.env_casing(),
            );
            (variant, attrs)
        })
        .partition(|(_, attrs)| {
            let kind = attrs.kind();
            match &*kind {
                Kind::Flatten => true,
                _ => false,
            }
        });

    let match_arms = variants.iter().map(|(variant, attrs)| {
        let sub_name = attrs.cased_name();
        let variant_name = &variant.ident;
        let constructor_block = match variant.fields {
            Named(ref fields) => from_argmatches::gen_constructor(&fields.named, &attrs),
            Unit => quote!(),
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote!( ( <#ty as ::clap::FromArgMatches>::from_arg_matches(matches) ) )
            }
            Unnamed(..) => abort_call_site!("{}: tuple enums are not supported", variant.ident),
        };

        quote! {
            (#sub_name, Some(matches)) => {
                Some(#name :: #variant_name #constructor_block)
            }
        }
    });
    let child_subcommands = flatten_variants.iter().map(|(variant, _attrs)| {
        let variant_name = &variant.ident;
        match variant.fields {
            Unnamed(ref fields) if fields.unnamed.len() == 1 => {
                let ty = &fields.unnamed[0];
                quote! {
                    if let Some(res) = <#ty as ::clap::Subcommand>::from_subcommand(other.0, other.1) {
                        return Some(#name :: #variant_name (res));
                    }
                }
            }
            _ => abort!(
                variant,
                "`flatten` is usable only with single-typed tuple variants"
            ),
        }
    });

    quote! {
        fn from_subcommand<'b>(
            name: &'b str,
            sub: Option<&'b ::clap::ArgMatches>) -> Option<Self>
        {
            match (name, sub) {
                #( #match_arms ),*,
                other => {
                    #( #child_subcommands )*;
                    None
                }
            }
        }
    }
}