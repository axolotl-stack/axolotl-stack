use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, Pat, PatType, Path, Type};

#[proc_macro_attribute]
pub fn event_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // This attribute is just a marker for indentation scanning;
    // the logic is handled in the `plugin` macro.
    // We just return the item as-is.
    item
}

#[proc_macro_attribute]
pub fn plugin(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    let mut handlers = Vec::new();
    let mut match_arms = Vec::new();

    // Filter items: keep trait methods in input, move handlers to 'handlers'
    let mut trait_items = Vec::new();

    for item in input.items.drain(..) {
        if let ImplItem::Fn(method) = item {
            let is_handler = method
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("event_handler"));

            if is_handler {
                let method_name = &method.sig.ident;
                // Parse the second argument to find the Event type
                if let Some(syn::FnArg::Typed(pat_type)) = method.sig.inputs.iter().nth(1) {
                    if let Type::Reference(type_ref) = &*pat_type.ty {
                        if let Type::Path(type_path) = &*type_ref.elem {
                            if let Some(ident) = type_path.path.get_ident() {
                                let struct_name = ident.to_string();
                                let arm = generate_match_arm(&struct_name, method_name);
                                if let Some(arm_code) = arm {
                                    match_arms.push(arm_code);
                                }
                            }
                        }
                    }
                }
                handlers.push(ImplItem::Fn(method));
            } else {
                trait_items.push(ImplItem::Fn(method));
            }
        } else {
            trait_items.push(item);
        }
    }

    input.items = trait_items;

    let expanded = quote! {
        // The trait implementation (only valid trait methods)
        #input

        // Inherent implementation for handlers
        impl #self_ty {
            #(#handlers)*
        }

        #[no_mangle]
        pub extern "C" fn on_tick(ptr: *mut u8, len: i32) -> u64 {
            // 1. Read events from guest memory
            let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            let events: Vec<unastar_api::PluginEvent> = borsh::from_slice(slice).unwrap_or_default();

            // 2. Process events
            static mut INSTANCE: Option<#self_ty> = None;

            let instance = unsafe {
                if INSTANCE.is_none() {
                    INSTANCE = Some(#self_ty::default());
                }
                INSTANCE.as_mut().unwrap()
            };

            // Create context wrapper with interior mutability
            let ctx = unastar_api::GameContext::new();

            for (event_id, event) in events.into_iter().enumerate() {
                match event {
                    #(#match_arms)*
                    _ => {}
                }
            }

            // 3. Serialize actions to return
            let actions = ctx.actions.borrow();
            let out_vec = borsh::to_vec(&*actions).unwrap_or_default();
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

fn generate_match_arm(
    struct_name: &str,
    method_name: &syn::Ident,
) -> Option<proc_macro2::TokenStream> {
    match struct_name {
        "ChatEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerChat { player, message } => {
                let event = unastar_api::ChatEvent {
                    player,
                    message,
                    event_id: event_id as u32,
                    ctx: &ctx
                };
                instance.#method_name(&event, &ctx);
            }
        }),
        "JoinEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerJoin { player, username } => {
                let event = unastar_api::JoinEvent { player, username };
                instance.#method_name(&event, &ctx);
            }
        }),
        "QuitEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerQuit { player } => {
                let event = unastar_api::QuitEvent { player };
                instance.#method_name(&event, &ctx);
            }
        }),
        "BlockBreakEvent" => Some(quote! {
            unastar_api::PluginEvent::BlockBreak { player, position, block_id } => {
                let event = unastar_api::BlockBreakEvent { player, position, block_id };
                instance.#method_name(&event, &ctx);
            }
        }),
        "BlockPlaceEvent" => Some(quote! {
            unastar_api::PluginEvent::BlockPlace { player, position, block_id } => {
                let event = unastar_api::BlockPlaceEvent { player, position, block_id };
                instance.#method_name(&event, &ctx);
            }
        }),
        "MoveEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerMove { player, from, to } => {
                let event = unastar_api::MoveEvent { player, from, to };
                instance.#method_name(&event, &ctx);
            }
        }),
        "JumpEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerJump { player } => {
                let event = unastar_api::JumpEvent { player };
                instance.#method_name(&event, &ctx);
            }
        }),
        "SneakEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerToggleSneak { player, is_sneaking } => {
                let event = unastar_api::SneakEvent { player, is_sneaking };
                instance.#method_name(&event, &ctx);
            }
        }),
        "SprintEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerToggleSprint { player, is_sprinting } => {
                let event = unastar_api::SprintEvent { player, is_sprinting };
                instance.#method_name(&event, &ctx);
            }
        }),
        "HeldSlotChangeEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerHeldSlotChange { player, old_slot, new_slot } => {
                let event = unastar_api::HeldSlotChangeEvent { player, old_slot, new_slot };
                instance.#method_name(&event, &ctx);
            }
        }),
        "StartBreakEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerStartBreak { player, position, face } => {
                let event = unastar_api::StartBreakEvent { player, position, face };
                instance.#method_name(&event, &ctx);
            }
        }),
        "InteractBlockEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerInteractBlock { player, position, face } => {
                let event = unastar_api::InteractBlockEvent { player, position, face };
                instance.#method_name(&event, &ctx);
            }
        }),
        "ItemUseEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerItemUse { player } => {
                let event = unastar_api::ItemUseEvent { player };
                instance.#method_name(&event, &ctx);
            }
        }),
        "SwingEvent" => Some(quote! {
            unastar_api::PluginEvent::PlayerSwing { player } => {
                let event = unastar_api::SwingEvent { player };
                instance.#method_name(&event, &ctx);
            }
        }),
        "TaskCompleteEvent" => Some(quote! {
            unastar_api::PluginEvent::TaskComplete { task_id, result } => {
                let event = unastar_api::TaskCompleteEvent { task_id, result };
                instance.#method_name(&event, &ctx);
            }
        }),
        // Timer and Tick events skipped for now or need their own structs
        _ => None,
    }
}

#[proc_macro_attribute]
pub fn native_plugin(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    // Check if implementing Plugin trait
    let is_trait_impl = input.trait_.is_some();
    if !is_trait_impl {
        return TokenStream::from(
            quote! { compile_error!("native_plugin can only be used on Plugin trait implementation"); },
        );
    }

    let mut inherent_meths = Vec::new();
    let mut trait_meths = Vec::new();

    // Map event type -> method implementation
    let mut event_impls = Vec::new();

    for item in input.items.drain(..) {
        if let ImplItem::Fn(mut method) = item {
            match method
                .attrs
                .iter()
                .position(|a| a.path().is_ident("event_handler"))
            {
                Some(index) => {
                    method.attrs.remove(index);

                    let sig_type = classify_native_handler(&method.sig);
                    let method_ident = &method.sig.ident;

                    if let Some(handler_type) = sig_type {
                        let impl_code = match handler_type {
                            "Chat" => quote! {
                                fn on_chat(&mut self, player: &mut unastar_api::native::Player, message: &str) -> bool {
                                    self.#method_ident(player, message)
                                }
                            },
                            "Move" => quote! {
                                fn on_player_move(&mut self, ctx: &mut unastar_api::native::NativeGameContext, entity: unastar_api::native::PluginEntity, old_pos: unastar_api::native::Vec3, new_pos: unastar_api::native::Vec3) {
                                    self.#method_ident(ctx, entity, old_pos, new_pos);
                                }
                            },
                            "Break" => quote! {
                                fn on_block_break(&mut self, ctx: &mut unastar_api::native::NativeGameContext, entity: unastar_api::native::PluginEntity, pos: unastar_api::native::BlockPos) -> bool {
                                    self.#method_ident(ctx, entity, pos)
                                }
                            },
                            "Place" => quote! {
                                fn on_block_place(&mut self, ctx: &mut unastar_api::native::NativeGameContext, entity: unastar_api::native::PluginEntity, pos: unastar_api::native::BlockPos, block_id: u32) -> bool {
                                    self.#method_ident(ctx, entity, pos, block_id)
                                }
                            },
                            "Join" => quote! {
                                fn on_player_join(&mut self, ctx: &mut unastar_api::native::NativeGameContext, entity: unastar_api::native::PluginEntity, username: &str) {
                                    self.#method_ident(ctx, entity, username);
                                }
                            },
                            "Quit" => quote! {
                                fn on_player_quit(&mut self, ctx: &mut unastar_api::native::NativeGameContext, entity: unastar_api::native::PluginEntity) {
                                    self.#method_ident(ctx, entity);
                                }
                            },
                            _ => quote! {},
                        };
                        event_impls.push(impl_code);
                    }

                    inherent_meths.push(ImplItem::Fn(method));
                }
                None => {
                    trait_meths.push(ImplItem::Fn(method));
                }
            }
        } else {
            trait_meths.push(item);
        }
    }

    let trait_path = &input.trait_.as_ref().unwrap().1;

    let output = quote! {
        impl #self_ty {
            #(#inherent_meths)*
        }

        impl #trait_path for #self_ty {
            #(#trait_meths)*
            #(#event_impls)*
        }
    };

    TokenStream::from(output)
}

fn classify_native_handler(sig: &syn::Signature) -> Option<&'static str> {
    // Skip receiver (self) if present
    let inputs: Vec<_> = sig
        .inputs
        .iter()
        .filter(|arg| matches!(arg, FnArg::Typed(_)))
        .collect();

    if let Some(FnArg::Typed(pat)) = inputs.first() {
        if is_matching_type(&pat.ty, "Player") {
            return Some("Chat");
        }
        if is_matching_type(&pat.ty, "NativeGameContext") || is_matching_type(&pat.ty, "Context") {
            // Check 3rd input (index 2 in filtered list)
            // Inputs: ctx, entity, ...
            if inputs.len() >= 3 {
                if let Some(FnArg::Typed(pat3)) = inputs.get(2) {
                    if is_matching_type(&pat3.ty, "Vec3") {
                        return Some("Move");
                    }
                    if is_matching_type(&pat3.ty, "BlockPos") {
                        if inputs.len() >= 4 {
                            return Some("Place");
                        }
                        return Some("Break");
                    }
                    if is_matching_type(&pat3.ty, "str") {
                        return Some("Join");
                    }
                }
            }
            if inputs.len() == 2 {
                return Some("Quit");
            }
        }
    }
    None
}

fn is_matching_type(ty: &Type, name: &str) -> bool {
    match ty {
        Type::Path(tp) => {
            if let Some(ident) = tp.path.get_ident() {
                return ident.to_string() == name;
            }
            // Check last segment
            if let Some(seg) = tp.path.segments.last() {
                return seg.ident.to_string() == name;
            }
            false
        }
        Type::Reference(tr) => is_matching_type(&tr.elem, name),
        _ => false,
    }
}
