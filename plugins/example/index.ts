import { JSON, JSONEncoder } from "assemblyscript-json/assembly/index";

@external("unastar", "player_get_info")
declare function player_get_info(handle: u32, ptr: usize, len: u32): u32;

@external("unastar", "world_get_spawn")
declare function world_get_spawn(ptr: usize, len: u32): u32;

// Allocator for Host to write to
export function alloc(size: i32): usize {
    return heap.alloc(size);
}

export function dealloc(ptr: usize, size: i32): void {
    heap.free(ptr);
}

export function on_tick(ptr: usize, len: i32): u64 {
    // Read memory into buffer
    let buffer = new Uint8Array(len);
    for (let i = 0; i < len; i++) {
        buffer[i] = load<u8>(ptr + i); // copy from raw ptr
    }

    let inputStr = String.UTF8.decode(buffer.buffer);

    let actions = new JSONEncoder();
    actions.pushArray(null);

    // Parse generic Array of events
    let json = JSON.parse(inputStr);

    // Debug: Log if Chat
    if (inputStr.includes("PlayerChat")) {
        actions.pushObject(null);
        actions.pushObject("Log");
        actions.setString("level", "info");
        actions.setString("message", "TS Received Chat JSON: " + inputStr);
        actions.popObject();
        actions.popObject();
    }

    if (json.isArr) {
        let events = (json as JSON.Arr).valueOf();
        for (let i = 0; i < events.length; i++) {
            let evtObj = events[i] as JSON.Obj;

            // Rust Serde enum with fields: {"PlayerChat": {...}}
            if (evtObj.has("PlayerChat")) {
                let chatObj = evtObj.getObj("PlayerChat");
                if (chatObj) {
                    let msg = chatObj.getString("message");

                    if (msg) {
                        let msgVal = msg.valueOf();

                        if (msgVal == "!whereami") {
                            // Handle Player Handle (Serialized as Integer)
                            let playerHandleVal = chatObj.getInteger("player");
                            if (playerHandleVal) {
                                let handle = playerHandleVal.valueOf() as u32;
                                let uuid = getPlayerUUID(handle);

                                if (uuid.length > 0) {
                                    // Send Message Action
                                    actions.pushObject(null);
                                    actions.pushObject("SendMessage");
                                    actions.setString("player_id", uuid);
                                    actions.setString("message", "Hello from TypeScript (AssemblyScript) Plugin!");
                                    actions.popObject();
                                    actions.popObject();
                                }
                            }
                        } else if (msgVal == "!bench_ts") {
                            // Log start
                            actions.pushObject(null);
                            actions.pushObject("Log");
                            actions.setString("level", "info");
                            actions.setString("message", "TS Bench Starting...");
                            actions.popObject();
                            actions.popObject();

                            // Benchmark 100 text calls
                            let start = Date.now();
                            let bufSize = 64;
                            let bufPtr = heap.alloc(bufSize);
                            for (let k = 0; k < 100; k++) {
                                world_get_spawn(bufPtr, bufSize);
                            }
                            heap.free(bufPtr);
                            let end = Date.now();
                            let duration = end - start;

                            // Log result
                            actions.pushObject(null);
                            actions.pushObject("Log");
                            actions.setString("level", "info");
                            actions.setString("message", "TS Bench: 100 world_get_spawn calls took " + duration.toString() + "ms");
                            actions.popObject();
                            actions.popObject();
                        }
                    }
                }
            }
        }
    }

    actions.popArray();
    let output = actions.serialize();

    // Return result buffer
    let outPtr = heap.alloc(output.length);
    for (let i = 0; i < output.length; i++) {
        store<u8>(outPtr + i, output[i]);
    }

    let packed: u64 = (u64(output.length) << 32) | u64(outPtr);
    return packed;
}

function getPlayerUUID(handle: u32): string {
    let bufSize = 512;
    let bufPtr = heap.alloc(bufSize);
    let len = player_get_info(handle, bufPtr, bufSize);

    if (len == 0) {
        heap.free(bufPtr);
        return "";
    }

    // Copy to array for decoding
    let data = new Uint8Array(len);
    for (let i = 0; i < i32(len); i++) {
        data[i] = load<u8>(bufPtr + i);
    }
    heap.free(bufPtr);

    let jsonStr = String.UTF8.decode(data.buffer);
    let parsed = JSON.parse(jsonStr);

    if (parsed.isObj) {
        let obj = parsed as JSON.Obj;
        let uuidStr = obj.getString("uuid");
        if (uuidStr) return uuidStr.valueOf();
    }
    return "";
}
