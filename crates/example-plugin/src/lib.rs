use unastar_api::{PluginAction, PluginEvent};
use std::mem::ManuallyDrop;

#[no_mangle]
pub extern "C" fn on_tick(ptr: *mut u8, len: i32) -> u64 {
    // 1. Read events from host
    let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let events: Vec<PluginEvent> = serde_json::from_slice(slice).unwrap_or_default();

    // 2. Process events
    let mut actions = Vec::new();
    for event in events {
        match event {
            PluginEvent::Tick { tick_id, .. } => {
                if tick_id % 200 == 0 {
                    actions.push(PluginAction::Log {
                        level: "info".to_string(),
                        message: format!("Plugin heartbeat (Tick {})", tick_id),
                    });
                }
            }
            PluginEvent::PlayerChat { player_id: _, message } => {
                actions.push(PluginAction::Log {
                    level: "info".to_string(),
                    message: format!("Chat intercepted: {}", message),
                });
            }
            PluginEvent::BlockBreak { player_id: _, x, y, z, block_name } => {
                actions.push(PluginAction::Log {
                    level: "info".to_string(),
                    message: format!("Block broken: {} at ({}, {}, {})", block_name, x, y, z),
                });

                if block_name.contains("stone") {
                    actions.push(PluginAction::SetBlock {
                        x, y, z,
                        block_name: "bedrock".to_string(),
                    });
                    actions.push(PluginAction::Log {
                        level: "warn".to_string(),
                        message: "Replaced stone with bedrock!".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    // 3. Serialize actions to return
    let mut out_vec = serde_json::to_vec(&actions).unwrap_or_default();
    let out_len = out_vec.len();
    let out_ptr = out_vec.as_mut_ptr();
    
    // Prevent deallocation
    std::mem::forget(out_vec);

    // Pack result: (len << 32) | ptr
    ((out_len as u64) << 32) | (out_ptr as u64)
}

// Allocator for the host to use
#[no_mangle]
pub extern "C" fn alloc(size: i32) -> *mut u8 {
    let mut vec = Vec::with_capacity(size as usize);
    let ptr = vec.as_mut_ptr();
    std::mem::forget(vec);
    ptr
}

// Deallocator for the host to use
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, size: i32) {
    let _ = Vec::from_raw_parts(ptr, size as usize, size as usize);
}

#[no_mangle]
pub extern "C" fn on_load() {
    // Optional initialization
}
