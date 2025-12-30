use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct};

#[proc_macro_attribute]
pub fn plugin(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;

    let expanded = quote! {
        #input

        #[no_mangle]
        pub extern "C" fn on_tick(ptr: *mut u8, len: i32) -> u64 {
            // 1. Read events from host
            let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            let events: Vec<unastar_api::PluginEvent> = serde_json::from_slice(slice).unwrap_or_default();

            // 2. Process events
            static mut INSTANCE: Option<#name> = None;

            let instance = unsafe {
                if INSTANCE.is_none() {
                    INSTANCE = Some(#name::default());
                }
                INSTANCE.as_mut().unwrap()
            };

            // Create context wrapper
            let ctx = unastar_api::GameContext;

            let actions = unastar_api::Plugin::on_tick(instance, events, &ctx);

            // 3. Serialize actions to return
            let out_vec = serde_json::to_vec(&actions).unwrap_or_default();
            let out_len = out_vec.len();
            let out_ptr = out_vec.as_ptr();

            // Prevent deallocation of the returned buffer by the plugin
            std::mem::forget(out_vec);

            // Pack result: (len << 32) | ptr
            ((out_len as u64) << 32) | (out_ptr as u64)
        }
    };

    TokenStream::from(expanded)
}
