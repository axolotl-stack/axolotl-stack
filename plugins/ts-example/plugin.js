export async function instantiate(module, imports = {}) {
  const __module0 = imports.unastar;
  const adaptedImports = {
    env: Object.setPrototypeOf({
      abort(message, fileName, lineNumber, columnNumber) {
        // ~lib/builtins/abort(~lib/string/String | null?, ~lib/string/String | null?, u32?, u32?) => void
        message = __liftString(message >>> 0);
        fileName = __liftString(fileName >>> 0);
        lineNumber = lineNumber >>> 0;
        columnNumber = columnNumber >>> 0;
        (() => {
          // @external.js
          throw Error(`${message} in ${fileName}:${lineNumber}:${columnNumber}`);
        })();
      },
      "Date.now"() {
        // ~lib/bindings/dom/Date.now() => f64
        return Date.now();
      },
    }, Object.assign(Object.create(globalThis), imports.env || {})),
    unastar: Object.setPrototypeOf({
      player_get_info(handle, ptr, len) {
        // assembly/index/player_get_info(u32, usize, u32) => u32
        handle = handle >>> 0;
        ptr = ptr >>> 0;
        len = len >>> 0;
        return __module0.player_get_info(handle, ptr, len);
      },
      world_get_spawn(ptr, len) {
        // assembly/index/world_get_spawn(usize, u32) => u32
        ptr = ptr >>> 0;
        len = len >>> 0;
        return __module0.world_get_spawn(ptr, len);
      },
    }, __module0),
  };
  const { exports } = await WebAssembly.instantiate(module, adaptedImports);
  const memory = exports.memory || imports.env.memory;
  const adaptedExports = Object.setPrototypeOf({
    alloc(size) {
      // assembly/index/alloc(i32) => usize
      return exports.alloc(size) >>> 0;
    },
    on_tick(ptr, len) {
      // assembly/index/on_tick(usize, i32) => u64
      return BigInt.asUintN(64, exports.on_tick(ptr, len));
    },
  }, exports);
  function __liftString(pointer) {
    if (!pointer) return null;
    const
      end = pointer + new Uint32Array(memory.buffer)[pointer - 4 >>> 2] >>> 1,
      memoryU16 = new Uint16Array(memory.buffer);
    let
      start = pointer >>> 1,
      string = "";
    while (end - start > 1024) string += String.fromCharCode(...memoryU16.subarray(start, start += 1024));
    return string + String.fromCharCode(...memoryU16.subarray(start, end));
  }
  return adaptedExports;
}
