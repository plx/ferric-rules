//! Procedural macros used to generate Ferric's panic-containing C exports.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, Pat, ReturnType, Type, TypePtr, Visibility};

/// Generate a C export wrapper around a non-extern Rust implementation.
///
/// The wrapper routes every call through `crate::boundary::invoke`, whose
/// return-type trait defines the panic sentinel. An `engine` pointer parameter
/// is detected mechanically so a contained panic can also update that
/// handle's diagnostic channel. Ownership-consuming free functions use
/// `#[ffi_export(global_only)]` because a panic may leave the handle lifetime
/// indeterminate.
#[proc_macro_attribute]
pub fn ffi_export(args: TokenStream, item: TokenStream) -> TokenStream {
    let global_only = match args.to_string().as_str() {
        "" => false,
        "global_only" => true,
        _ => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "expected #[ffi_export] or #[ffi_export(global_only)]",
            )
            .into_compile_error()
            .into();
        }
    };

    let function = parse_macro_input!(item as ItemFn);
    expand_ffi_export(function, global_only)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_ffi_export(
    mut function: ItemFn,
    global_only: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    if !matches!(function.vis, Visibility::Public(_)) {
        return Err(syn::Error::new_spanned(
            &function.vis,
            "FFI export must be public",
        ));
    }
    let Some(abi) = function.sig.abi.as_ref() else {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "FFI export must use extern \"C\"",
        ));
    };
    let Some(abi_name) = abi.name.as_ref() else {
        return Err(syn::Error::new_spanned(abi, "FFI export ABI must be C"));
    };
    if abi_name.value() != "C" {
        return Err(syn::Error::new_spanned(
            abi_name,
            "FFI export ABI must be C",
        ));
    }

    let mut wrapper_attrs = std::mem::take(&mut function.attrs);
    wrapper_attrs.retain(|attribute| !attribute.path().is_ident("no_mangle"));
    let implementation_attrs: Vec<_> = wrapper_attrs
        .iter()
        .filter(|attribute| !attribute.path().is_ident("doc"))
        .collect();
    let visibility = function.vis;
    let wrapper_signature = function.sig;
    let implementation_body = function.block;
    let export_name = wrapper_signature.ident.clone();
    let implementation_name = format_ident!("__ferric_ffi_impl_{export_name}");

    let argument_names = argument_names(&wrapper_signature.inputs)?;
    let target = if global_only {
        quote!(crate::boundary::PanicTarget::Global)
    } else {
        infer_panic_target(&wrapper_signature.inputs)?
    };

    let mut implementation_signature = wrapper_signature.clone();
    implementation_signature.ident = implementation_name.clone();
    implementation_signature.abi = None;

    let call = if implementation_signature.unsafety.is_some() {
        quote!(unsafe { #implementation_name(#(#argument_names),*) })
    } else {
        quote!(#implementation_name(#(#argument_names),*))
    };

    let return_type = match &wrapper_signature.output {
        ReturnType::Default => quote!(()),
        ReturnType::Type(_, ty) => quote!(#ty),
    };

    Ok(quote! {
        #(#wrapper_attrs)*
        #[no_mangle]
        #visibility #wrapper_signature {
            crate::boundary::invoke::<#return_type, _>(
                stringify!(#export_name),
                #target,
                move || #call,
            )
        }

        #(#implementation_attrs)*
        #implementation_signature #implementation_body
    })
}

fn argument_names(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::token::Comma>,
) -> syn::Result<Vec<syn::Ident>> {
    inputs
        .iter()
        .map(|argument| match argument {
            FnArg::Typed(argument) => match argument.pat.as_ref() {
                Pat::Ident(ident) if ident.subpat.is_none() => Ok(ident.ident.clone()),
                pattern => Err(syn::Error::new_spanned(
                    pattern,
                    "FFI export arguments must use simple identifier patterns",
                )),
            },
            FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
                receiver,
                "FFI exports cannot have a receiver",
            )),
        })
        .collect()
}

fn infer_panic_target(
    inputs: &syn::punctuated::Punctuated<FnArg, syn::token::Comma>,
) -> syn::Result<proc_macro2::TokenStream> {
    let Some(engine_argument) = inputs.iter().find_map(|argument| match argument {
        FnArg::Typed(argument)
            if matches!(argument.pat.as_ref(), Pat::Ident(ident) if ident.ident == "engine") =>
        {
            Some(argument)
        }
        _ => None,
    }) else {
        return Ok(quote!(crate::boundary::PanicTarget::Global));
    };

    let Type::Ptr(TypePtr { elem, .. }) = engine_argument.ty.as_ref() else {
        return Err(syn::Error::new_spanned(
            &engine_argument.ty,
            "engine must be a raw pointer",
        ));
    };
    let Type::Path(engine_type) = elem.as_ref() else {
        return Err(syn::Error::new_spanned(
            elem,
            "engine pointer must target a named handle type",
        ));
    };
    let Some(type_name) = engine_type
        .path
        .segments
        .last()
        .map(|segment| &segment.ident)
    else {
        return Err(syn::Error::new_spanned(
            engine_type,
            "engine pointer type is empty",
        ));
    };

    if type_name == "FerricEngine" {
        Ok(quote!(crate::boundary::PanicTarget::RawEngine(
            engine as *const crate::engine::FerricEngine,
        )))
    } else if type_name == "FerricPinnedEngine" {
        Ok(quote!(crate::boundary::PanicTarget::PinnedEngine(
            engine as *const crate::pinned::FerricPinnedEngine,
        )))
    } else {
        Err(syn::Error::new_spanned(
            type_name,
            "unsupported engine handle type; use #[ffi_export(global_only)] if appropriate",
        ))
    }
}
