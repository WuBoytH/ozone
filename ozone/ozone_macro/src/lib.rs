use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, ToTokens};
use syn::{parse::Parse, parse_macro_input, spanned::Spanned};

struct TypeAssertAttributes(syn::LitInt);

impl TypeAssertAttributes {
    fn parse_attr(attr: &syn::Attribute, name: &str) -> syn::Result<Self> {
        let path = attr.path();

        match path.get_ident() {
            Some(ident) if ident.to_string() == name => {},
            _ => return Err(syn::Error::new(attr.meta.span(), "Type assert attribute requires 'ta' ident")),
        }

        syn::parse(attr.meta.require_name_value()?.to_token_stream().into())
    }
}

impl Parse for TypeAssertAttributes {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let _: syn::Path = input.parse()?;
        let _: syn::Token![=] = input.parse()?;

        input.parse().map(|int| Self(int))
    }
}

#[proc_macro_derive(TypeAssert, attributes(size, off, offset))]
pub fn derive_type_assertion(item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as syn::ItemStruct);

    let mut size = None;

    for attr in item_struct.attrs.iter() {
        if let Ok(attr) = TypeAssertAttributes::parse_attr(attr, "size") {
            size = Some(attr.0);
            break
        }
    }

    let size = match size {
        Some(int) => int,
        None => {
            return syn::Error::new(
                item_struct.ident.span(),
                "TypeAssert structure must have 'ta' attribute to specify the size",
            )
            .into_compile_error()
            .into()
        },
    };

    let fields: Vec<(&syn::Ident, syn::LitInt)> = item_struct
        .fields
        .iter()
        .filter_map(|field| {
            if field.ident.is_none() {
                return None
            }

            let mut attributes = field.attrs.iter();

            loop {
                match attributes.next() {
                    Some(attr) => {
                        match TypeAssertAttributes::parse_attr(attr, "off") {
                            Ok(attr) => break Some((field.ident.as_ref().unwrap(), attr.0)),
                            _ => {
                                match TypeAssertAttributes::parse_attr(attr, "offset") {
                                    Ok(attr) => break Some((field.ident.as_ref().unwrap(), attr.0)),
                                    _ => {},
                                }
                            },
                        }
                    },
                    None => break None,
                }
            }
        })
        .collect();

    let ty_ident = &item_struct.ident;

    let exprs = fields
        .into_iter()
        .map(|(ident, val)| {
            syn::parse_quote! {
                assert_eq!(
                    offset_of!(#ty_ident, #ident),
                    #val,
                    "The offset of {}.{} must be {:#x}, but it is {:#x}",
                    stringify!(#ty_ident),
                    stringify!(#ident),
                    #val,
                    offset_of!(#ty_ident, #ident)
                );
            }
        })
        .collect::<Vec<syn::Stmt>>()
        .into_iter();

    let test_mod_name = quote::format_ident!("{}_tests", ty_ident);

    quote::quote!(
        impl #ty_ident {
            pub fn assert(is_size_assert: bool) {
                if is_size_assert {
                    assert_eq!(size_of!(#ty_ident), #size, "The size of {} must be {:#x}, but it is {:#x}", stringify!(#ty_ident), #size, size_of!(#ty_ident));
                }
                #(
                    #exprs
                )*
            }
        }

        #[cfg(feature = "type_assert")]
        #[allow(non_snake_case)]
        mod #test_mod_name {
            use super::*;

            #[test]
            pub fn check_size_field_bounds() {
                #ty_ident::assert(false);
                #ty_ident::assert(true);
            }
        }
    ).into()
}

#[proc_macro_attribute]
pub fn main(_: TokenStream, item: TokenStream) -> TokenStream {
    let mut main_function = parse_macro_input!(item as syn::ItemFn);

    // extern "C"
    main_function.sig.abi = Some(syn::Abi {
        extern_token: syn::token::Extern { span: Span::call_site() },
        name: Some(syn::LitStr::new("C", Span::call_site())),
    });

    let mut output = TokenStream2::new();

    quote!(
        std::arch::global_asm!("
        .section .nro_header
        .global __nro_header_start
        .global __module_start
        __module_start:
        .word 0
        .word _mod_header
        .word 0
        .word 0

        .section .rodata.module_name
        .word 0
        .word 5
        .ascii \"ozone\"
        .section .rodata.mod0
        .global _mod_header
        _mod_header:
            .ascii \"MOD0\"
            .word __dynamic_start - _mod_header
            .word __bss_start - _mod_header
            .word __bss_end - _mod_header
            .word __eh_frame_hdr_start - _mod_header
            .word __eh_frame_hdr_end - _mod_header
            .word __nx_module_runtime - _mod_header // runtime-generated module object offset
        .global IS_NRO
        IS_NRO:
            .word 1

        .section .bss.module_runtime
        __nx_module_runtime:
        .space 0xD0
        ");
    )
    .to_tokens(&mut output);

    quote!(
        // this is both fine and normal and don't think too hard about it
        const _: fn() = || {
            use ::skyline::libc::{pthread_mutex_t, pthread_key_t, pthread_t, c_int, c_void};

            // re-export pthread_mutex_lock as __pthread_mutex_lock
            //
            // this is done in order to fix the fact that switch libstd depends on libc-nnsdk
            // which itself links against symbol aliases only present in certain versions of
            // nnsdk.
            #[export_name = "__pthread_mutex_lock"]
            pub unsafe extern "C" fn _skyline_internal_pthread_mutex_lock_shim(lock: *mut pthread_mutex_t) -> c_int {
                extern "C" {
                    fn pthread_mutex_lock(lock: *mut pthread_mutex_t) -> c_int;
                }

                pthread_mutex_lock(lock)
            }

            #[export_name = "__pthread_key_create"]
            pub unsafe extern "C" fn _skyline_internal_pthread_key_create_shim(key: *mut pthread_key_t, func: extern fn(*mut c_void)) -> c_int {
                extern "C" {
                    fn pthread_key_create(
                        key: *mut pthread_key_t, func: extern fn(*mut c_void)
                    ) -> c_int;
                }

                pthread_key_create(key, func)
            }

            #[export_name = "__pthread_key_delete"]
            pub unsafe extern "C" fn _skyline_internal_pthread_key_delete_shim(key: pthread_key_t) -> c_int {
                extern "C" {
                    fn pthread_key_delete(
                        key: pthread_key_t
                    ) -> c_int;
                }

                pthread_key_delete(key)
            }

            #[export_name = "__pthread_join"]
            pub unsafe extern "C" fn _skyline_internal_pthread_join_shim(native: pthread_t, value: *mut *mut c_void) -> c_int {
                extern "C" {
                    fn pthread_join(
                        native: pthread_t,
                        value: *mut *mut c_void,
                    ) -> c_int;
                }

                pthread_join(native, value)
            }
        };

        #output
        #main_function
    )
    .into()
}
