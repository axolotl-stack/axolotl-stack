declare namespace __AdaptedExports {
  /** Exported memory */
  export const memory: WebAssembly.Memory;
  /**
   * assembly/index/alloc
   * @param size `i32`
   * @returns `usize`
   */
  export function alloc(size: number): number;
  /**
   * assembly/index/dealloc
   * @param ptr `usize`
   * @param size `i32`
   */
  export function dealloc(ptr: number, size: number): void;
  /**
   * assembly/index/on_tick
   * @param ptr `usize`
   * @param len `i32`
   * @returns `u64`
   */
  export function on_tick(ptr: number, len: number): bigint;
}
/** Instantiates the compiled WebAssembly module with the given imports. */
export declare function instantiate(module: WebAssembly.Module, imports: {
  env: unknown,
  unastar: unknown,
}): Promise<typeof __AdaptedExports>;
