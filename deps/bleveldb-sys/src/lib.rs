//! Rust bindings for LevelDB
//!
//! This crate provides safe Rust bindings for LevelDB, a fast key-value storage library
//! written by Google, forked by Mojang. It exposes the C API through FFI with proper memory management
//! and error handling.
//!
//! # Safety
//!
//! All functions in this crate are marked `unsafe` as they interact with C code and
//! require proper pointer management. Callers must ensure:
//! - All pointers passed to functions are valid
//! - Memory is properly managed and freed
//! - Error pointers are checked after operations
//!

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

extern crate libc;
#[macro_use]
extern crate ffi_opaque;

use libc::size_t;
use libc::{c_char, c_int, c_uchar, c_void};

// Opaque type definitions for LevelDB objects
opaque! {
    /// Database handle representing an open LevelDB database instance
    pub struct leveldb_t;

    /// Cache for uncompressed data blocks to improve read performance
    pub struct leveldb_cache_t;

    /// Custom comparator for key ordering
    pub struct leveldb_comparator_t;

    /// Environment abstraction for OS-specific operations
    pub struct leveldb_env_t;

    /// File lock handle for concurrent access control
    pub struct leveldb_filelock_t;

    /// Filter policy for bloom filters and other filtering mechanisms
    pub struct leveldb_filterpolicy_t;

    /// Iterator for traversing database entries
    pub struct leveldb_iterator_t;

    /// Logger for informational and error messages
    pub struct leveldb_logger_t;

    /// Database configuration options
    pub struct leveldb_options_t;

    /// Random access file handle
    pub struct leveldb_randomfile_t;

    /// Read operation options and settings
    pub struct leveldb_readoptions_t;

    /// Sequential file handle
    pub struct leveldb_seqfile_t;

    /// Immutable database snapshot for consistent reads
    pub struct leveldb_snapshot_t;

    /// Writable file handle
    pub struct leveldb_writablefile_t;

    /// Batch of write operations for atomic updates
    pub struct leveldb_writebatch_t;

    /// Write operation options and settings
    pub struct leveldb_writeoptions_t;
}

/// Compression algorithms supported by LevelDB
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Compression {
    /// No compression - faster but uses more disk space
    No = 0,
    /// Snappy compression - good balance of speed and compression ratio
    Snappy = 1,
    /// Zstd compression - higher compression ratio, slower
    Zstd = 2,
    /// Zlib raw compression - compatible with zlib, slower
    ZlibRaw = 4,
}

unsafe extern "C" {
    // =========================================================================
    // Database Operations
    // =========================================================================

    /// Open a LevelDB database with the specified options
    ///
    /// # Arguments
    ///
    /// * `options` - Database configuration options
    /// * `name` - Path to the database directory as a null-terminated C string
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Returns
    ///
    /// Pointer to a new database handle on success, null pointer on failure
    ///
    /// # Safety
    ///
    /// - `options` must be a valid pointer from `leveldb_options_create()`
    /// - `name` must be a valid null-terminated C string
    /// - `errptr` must point to a valid `*mut c_char` location
    /// - Caller must call `leveldb_close()` on the returned pointer
    pub fn leveldb_open(
        options: *const leveldb_options_t,
        name: *const c_char,
        errptr: *mut *mut c_char,
    ) -> *mut leveldb_t;

    /// Close a database and release all associated resources
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle to close
    ///
    /// # Safety
    ///
    /// - `db` must be a valid pointer from `leveldb_open()`
    /// - After this call, `db` becomes invalid and should not be used
    pub fn leveldb_close(db: *mut leveldb_t);

    /// Store a key-value pair in the database
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `options` - Write options
    /// * `key` - Key data pointer
    /// * `keylen` - Length of key data in bytes
    /// * `val` - Value data pointer
    /// * `vallen` - Length of value data in bytes
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Safety
    ///
    /// - All pointers must be valid
    /// - `key` and `val` must point to at least `keylen` and `vallen` bytes respectively
    pub fn leveldb_put(
        db: *mut leveldb_t,
        options: *const leveldb_writeoptions_t,
        key: *const c_char,
        keylen: size_t,
        val: *const c_char,
        vallen: size_t,
        errptr: *mut *mut c_char,
    );

    /// Delete a key from the database
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `options` - Write options
    /// * `key` - Key data pointer
    /// * `keylen` - Length of key data in bytes
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Safety
    ///
    /// - All pointers must be valid
    /// - `key` must point to at least `keylen` bytes
    pub fn leveldb_delete(
        db: *mut leveldb_t,
        options: *const leveldb_writeoptions_t,
        key: *const c_char,
        keylen: size_t,
        errptr: *mut *mut c_char,
    );

    /// Execute a batch of write operations atomically
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `options` - Write options
    /// * `batch` - Batch of write operations to execute
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Safety
    ///
    /// - All pointers must be valid
    /// - `batch` must be a valid write batch
    pub fn leveldb_write(
        db: *mut leveldb_t,
        options: *const leveldb_writeoptions_t,
        batch: *mut leveldb_writebatch_t,
        errptr: *mut *mut c_char,
    );

    /// Retrieve a value for the given key
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `options` - Read options
    /// * `key` - Key data pointer
    /// * `keylen` - Length of key data in bytes
    /// * `vallen` - Pointer to store the length of returned value
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Returns
    ///
    /// Pointer to the value data on success, null pointer if key not found.
    /// Caller must call `leveldb_free()` on the returned pointer.
    ///
    /// # Safety
    ///
    /// - All pointers must be valid
    /// - Caller must free returned value with `leveldb_free()`
    pub fn leveldb_get(
        db: *mut leveldb_t,
        options: *const leveldb_readoptions_t,
        key: *const c_char,
        keylen: size_t,
        vallen: *mut size_t,
        errptr: *mut *mut c_char,
    ) -> *mut c_char;

    /// Create an iterator for traversing database entries
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `options` - Read options
    ///
    /// # Returns
    ///
    /// Pointer to a new iterator handle
    ///
    /// # Safety
    ///
    /// - `db` must be a valid database handle
    /// - Caller must call `leveldb_iter_destroy()` on the returned pointer
    pub fn leveldb_create_iterator(
        db: *mut leveldb_t,
        options: *const leveldb_readoptions_t,
    ) -> *mut leveldb_iterator_t;

    /// Create a snapshot of the current database state
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    ///
    /// # Returns
    ///
    /// Pointer to a new snapshot handle
    ///
    /// # Safety
    ///
    /// - `db` must be a valid database handle
    /// - Caller must call `leveldb_release_snapshot()` on the returned pointer
    pub fn leveldb_create_snapshot(db: *mut leveldb_t) -> *mut leveldb_snapshot_t;

    /// Release a snapshot and its associated resources
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `snapshot` - Snapshot to release
    ///
    /// # Safety
    ///
    /// - Both `db` and `snapshot` must be valid pointers
    /// - After this call, `snapshot` becomes invalid
    pub fn leveldb_release_snapshot(db: *mut leveldb_t, snapshot: *const leveldb_snapshot_t);

    /// Get property value from the database
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `propname` - Property name as null-terminated C string
    ///
    /// # Returns
    ///
    /// Pointer to property value string, or null if property doesn't exist.
    /// Caller must call `leveldb_free()` on the returned pointer.
    ///
    /// # Safety
    ///
    /// - `db` must be a valid database handle
    /// - `propname` must be a valid null-terminated C string
    /// - Caller must free returned value with `leveldb_free()`
    pub fn leveldb_property_value(db: *mut leveldb_t, propname: *const c_char) -> *mut c_char;

    /// Estimate sizes of key ranges in the database
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `num_ranges` - Number of key ranges to estimate
    /// * `range_start_key` - Array of range start keys
    /// * `range_start_key_len` - Array of start key lengths
    /// * `range_limit_key` - Array of range limit keys
    /// * `range_limit_key_len` - Array of limit key lengths
    /// * `sizes` - Output array for estimated sizes
    ///
    /// # Safety
    ///
    /// - All pointers must be valid and arrays must have proper lengths
    pub fn leveldb_approximate_sizes(
        db: *mut leveldb_t,
        num_ranges: c_int,
        range_start_key: *const *const c_char,
        range_start_key_len: *const size_t,
        range_limit_key: *const *const c_char,
        range_limit_key_len: *const size_t,
        sizes: *mut u64,
    );

    /// Compact the specified key range in the database
    ///
    /// # Arguments
    ///
    /// * `db` - Database handle
    /// * `start_key` - Start key of range to compact (inclusive)
    /// * `start_key_len` - Length of start key
    /// * `limit_key` - Limit key of range to compact (exclusive)
    /// * `limit_key_len` - Length of limit key
    ///
    /// # Safety
    ///
    /// - `db` must be a valid database handle
    /// - Key pointers must be valid if non-null
    pub fn leveldb_compact_range(
        db: *mut leveldb_t,
        start_key: *const c_char,
        start_key_len: size_t,
        limit_key: *const c_char,
        limit_key_len: size_t,
    );

    // =========================================================================
    // Management Operations
    // =========================================================================

    /// Destroy the contents of the specified database
    ///
    /// # Arguments
    ///
    /// * `options` - Options for the operation
    /// * `name` - Path to database directory
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Safety
    ///
    /// - `options` must be a valid options pointer
    /// - `name` must be a valid null-terminated C string
    pub fn leveldb_destroy_db(
        options: *const leveldb_options_t,
        name: *const c_char,
        errptr: *mut *mut c_char,
    );

    /// Repair a corrupted database
    ///
    /// # Arguments
    ///
    /// * `options` - Options for the operation
    /// * `name` - Path to database directory
    /// * `errptr` - Pointer to store error message if operation fails
    ///
    /// # Safety
    ///
    /// - `options` must be a valid options pointer
    /// - `name` must be a valid null-terminated C string
    pub fn leveldb_repair_db(
        options: *const leveldb_options_t,
        name: *const c_char,
        errptr: *mut *mut c_char,
    );

    // =========================================================================
    // Iterator Operations
    // =========================================================================

    /// Destroy an iterator and release its resources
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to destroy
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_destroy(it: *mut leveldb_iterator_t);

    /// Check if iterator is positioned at a valid entry
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to check
    ///
    /// # Returns
    ///
    /// Non-zero if iterator is valid, zero otherwise
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_valid(it: *const leveldb_iterator_t) -> c_uchar;

    /// Position iterator at the first key in the database
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to position
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_seek_to_first(it: *mut leveldb_iterator_t);

    /// Position iterator at the last key in the database
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to position
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_seek_to_last(it: *mut leveldb_iterator_t);

    /// Position iterator at the specified key
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to position
    /// * `k` - Key to seek to
    /// * `klen` - Length of key in bytes
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    /// - `k` must point to at least `klen` bytes
    pub fn leveldb_iter_seek(it: *mut leveldb_iterator_t, k: *const c_char, klen: size_t);

    /// Advance iterator to the next key
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to advance
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_next(it: *mut leveldb_iterator_t);

    /// Move iterator to the previous key
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator to move
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_prev(it: *mut leveldb_iterator_t);

    /// Get the key at current iterator position
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator
    /// * `klen` - Pointer to store key length
    ///
    /// # Returns
    ///
    /// Pointer to key data
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator positioned at a valid entry
    /// - The returned pointer is only valid until the iterator is modified
    pub fn leveldb_iter_key(it: *const leveldb_iterator_t, klen: *mut size_t) -> *const c_char;

    /// Get the value at current iterator position
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator
    /// * `vlen` - Pointer to store value length
    ///
    /// # Returns
    ///
    /// Pointer to value data
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator positioned at a valid entry
    /// - The returned pointer is only valid until the iterator is modified
    pub fn leveldb_iter_value(it: *const leveldb_iterator_t, vlen: *mut size_t) -> *const c_char;

    /// Get any error associated with the iterator
    ///
    /// # Arguments
    ///
    /// * `it` - Iterator
    /// * `errptr` - Pointer to store error message
    ///
    /// # Safety
    ///
    /// - `it` must be a valid iterator pointer
    pub fn leveldb_iter_get_error(it: *const leveldb_iterator_t, errptr: *mut *mut c_char);

    // =========================================================================
    // Write Batch Operations
    // =========================================================================

    /// Create a new write batch
    ///
    /// # Returns
    ///
    /// Pointer to a new write batch handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_writebatch_destroy()` on the returned pointer
    pub fn leveldb_writebatch_create() -> *mut leveldb_writebatch_t;

    /// Destroy a write batch and release its resources
    ///
    /// # Arguments
    ///
    /// * `b` - Write batch to destroy
    ///
    /// # Safety
    ///
    /// - `b` must be a valid write batch pointer
    pub fn leveldb_writebatch_destroy(b: *mut leveldb_writebatch_t);

    /// Clear all operations from a write batch
    ///
    /// # Arguments
    ///
    /// * `b` - Write batch to clear
    ///
    /// # Safety
    ///
    /// - `b` must be a valid write batch pointer
    pub fn leveldb_writebatch_clear(b: *mut leveldb_writebatch_t);

    /// Add a put operation to the write batch
    ///
    /// # Arguments
    ///
    /// * `b` - Write batch
    /// * `key` - Key data pointer
    /// * `keylen` - Length of key data in bytes
    /// * `val` - Value data pointer
    /// * `vallen` - Length of value data in bytes
    ///
    /// # Safety
    ///
    /// - `b` must be a valid write batch pointer
    /// - `key` and `val` must point to at least `keylen` and `vallen` bytes respectively
    pub fn leveldb_writebatch_put(
        b: *mut leveldb_writebatch_t,
        key: *const c_char,
        keylen: size_t,
        val: *const c_char,
        vallen: size_t,
    );

    /// Add a delete operation to the write batch
    ///
    /// # Arguments
    ///
    /// * `b` - Write batch
    /// * `key` - Key data pointer
    /// * `keylen` - Length of key data in bytes
    ///
    /// # Safety
    ///
    /// - `b` must be a valid write batch pointer
    /// - `key` must point to at least `keylen` bytes
    pub fn leveldb_writebatch_delete(
        b: *mut leveldb_writebatch_t,
        key: *const c_char,
        keylen: size_t,
    );

    /// Iterate over all operations in a write batch
    ///
    /// # Arguments
    ///
    /// * `b` - Write batch to iterate over
    /// * `state` - User state pointer passed to callback functions
    /// * `put` - Callback function for put operations
    /// * `deleted` - Callback function for delete operations
    ///
    /// # Safety
    ///
    /// - `b` must be a valid write batch pointer
    /// - Callback functions must have correct signatures
    pub fn leveldb_writebatch_iterate(
        b: *mut leveldb_writebatch_t,
        state: *mut c_void,
        put: extern "C" fn(*mut c_void, *const c_char, size_t, *const c_char, size_t),
        deleted: extern "C" fn(*mut c_void, *const c_char, size_t),
    );

    // =========================================================================
    // Options Management
    // =========================================================================

    /// Create a new database options handle
    ///
    /// # Returns
    ///
    /// Pointer to a new options handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_options_destroy()` on the returned pointer
    pub fn leveldb_options_create() -> *mut leveldb_options_t;

    /// Destroy an options handle and release its resources
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle to destroy
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_destroy(o: *mut leveldb_options_t);

    /// Set a custom comparator for key ordering
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `c` - Comparator to use
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_options_set_comparator(o: *mut leveldb_options_t, c: *mut leveldb_comparator_t);

    /// Set a filter policy for bloom filters
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `policy` - Filter policy to use
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_options_set_filter_policy(
        o: *mut leveldb_options_t,
        policy: *mut leveldb_filterpolicy_t,
    );

    /// Configure whether to create the database if it doesn't exist
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `val` - Non-zero to create if missing, zero otherwise
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_create_if_missing(o: *mut leveldb_options_t, val: c_uchar);

    /// Configure whether to error if the database already exists
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `val` - Non-zero to error if exists, zero otherwise
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_error_if_exists(o: *mut leveldb_options_t, val: c_uchar);

    /// Enable or disable paranoid checks
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `val` - Non-zero to enable paranoid checks, zero to disable
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_paranoid_checks(o: *mut leveldb_options_t, val: c_uchar);

    /// Set a custom environment for OS operations
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `env` - Environment to use
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_options_set_env(o: *mut leveldb_options_t, env: *mut leveldb_env_t);

    /// Set a custom logger for informational messages
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `logger` - Logger to use
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_options_set_info_log(o: *mut leveldb_options_t, logger: *mut leveldb_logger_t);

    /// Set the size of the write buffer
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `size` - Write buffer size in bytes
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_write_buffer_size(o: *mut leveldb_options_t, size: size_t);

    /// Set the maximum number of open files
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `num` - Maximum number of open files
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_max_open_files(o: *mut leveldb_options_t, num: c_int);

    /// Set the block cache for uncompressed data
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `cache` - Cache to use
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_options_set_cache(o: *mut leveldb_options_t, cache: *mut leveldb_cache_t);

    /// Set the size of data blocks
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `size` - Block size in bytes
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_block_size(o: *mut leveldb_options_t, size: size_t);

    /// Set the block restart interval
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `interval` - Restart interval
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_block_restart_interval(o: *mut leveldb_options_t, interval: c_int);

    /// Set the maximum file size for SST files
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `size` - Maximum file size in bytes
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_max_file_size(o: *mut leveldb_options_t, size: size_t);

    /// Enable or disable seek-based auto compaction
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `val` - Non-zero to disable seek auto compaction, zero to enable
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_disable_seek_autocompaction(o: *mut leveldb_options_t, val: c_uchar);

    /// Set the compression algorithm
    ///
    /// # Arguments
    ///
    /// * `o` - Options handle
    /// * `val` - Compression algorithm to use
    ///
    /// # Safety
    ///
    /// - `o` must be a valid options pointer
    pub fn leveldb_options_set_compression(o: *mut leveldb_options_t, val: Compression);

    // =========================================================================
    // Comparator Operations
    // =========================================================================

    /// Create a custom comparator
    ///
    /// # Arguments
    ///
    /// * `state` - User state pointer
    /// * `destructor` - Function to clean up state
    /// * `compare` - Function to compare two keys
    /// * `name` - Function to return comparator name
    ///
    /// # Returns
    ///
    /// Pointer to a new comparator handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_comparator_destroy()` on the returned pointer
    /// - Function pointers must be valid and have correct signatures
    pub fn leveldb_comparator_create(
        state: *mut c_void,
        destructor: extern "C" fn(*mut c_void),
        compare: extern "C" fn(*mut c_void, *const c_char, size_t, *const c_char, size_t) -> c_int,
        name: extern "C" fn(*mut c_void) -> *const c_char,
    ) -> *mut leveldb_comparator_t;

    /// Destroy a comparator and release its resources
    ///
    /// # Arguments
    ///
    /// * `c` - Comparator to destroy
    ///
    /// # Safety
    ///
    /// - `c` must be a valid comparator pointer
    pub fn leveldb_comparator_destroy(c: *mut leveldb_comparator_t);

    // =========================================================================
    // Filter Policy Operations
    // =========================================================================

    /// Create a custom filter policy
    ///
    /// # Arguments
    ///
    /// * `state` - User state pointer
    /// * `destructor` - Function to clean up state
    /// * `create_filter` - Function to create filter from keys
    /// * `key_may_match` - Function to check if key may be in filter
    /// * `name` - Function to return filter policy name
    ///
    /// # Returns
    ///
    /// Pointer to a new filter policy handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_filterpolicy_destroy()` on the returned pointer
    /// - Function pointers must be valid and have correct signatures
    pub fn leveldb_filterpolicy_create(
        state: *mut c_void,
        destructor: extern "C" fn(*mut c_void),
        create_filter: extern "C" fn(
            *mut c_void,
            *const *const c_char,
            *const size_t,
            c_int,
            *mut size_t,
        ) -> *mut c_char,
        key_may_match: extern "C" fn(
            *mut c_void,
            *const c_char,
            size_t,
            *const c_char,
            size_t,
        ) -> u8,
        name: extern "C" fn(*mut c_void) -> *const c_char,
    ) -> *mut leveldb_filterpolicy_t;

    /// Destroy a filter policy and release its resources
    ///
    /// # Arguments
    ///
    /// * `policy` - Filter policy to destroy
    ///
    /// # Safety
    ///
    /// - `policy` must be a valid filter policy pointer
    pub fn leveldb_filterpolicy_destroy(policy: *mut leveldb_filterpolicy_t);

    /// Create a bloom filter policy
    ///
    /// # Arguments
    ///
    /// * `bits_per_key` - Number of bits to use per key in the bloom filter
    ///
    /// # Returns
    ///
    /// Pointer to a new bloom filter policy handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_filterpolicy_destroy()` on the returned pointer
    pub fn leveldb_filterpolicy_create_bloom(bits_per_key: c_int) -> *mut leveldb_filterpolicy_t;

    // =========================================================================
    // Read Options Operations
    // =========================================================================

    /// Create a new read options handle
    ///
    /// # Returns
    ///
    /// Pointer to a new read options handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_readoptions_destroy()` on the returned pointer
    pub fn leveldb_readoptions_create() -> *mut leveldb_readoptions_t;

    /// Destroy a read options handle and release its resources
    ///
    /// # Arguments
    ///
    /// * `o` - Read options handle to destroy
    ///
    /// # Safety
    ///
    /// - `o` must be a valid read options pointer
    pub fn leveldb_readoptions_destroy(o: *mut leveldb_readoptions_t);

    /// Configure whether to verify checksums on read
    ///
    /// # Arguments
    ///
    /// * `o` - Read options handle
    /// * `val` - Non-zero to verify checksums, zero to skip
    ///
    /// # Safety
    ///
    /// - `o` must be a valid read options pointer
    pub fn leveldb_readoptions_set_verify_checksums(o: *mut leveldb_readoptions_t, val: c_uchar);

    /// Configure whether to fill the cache on read
    ///
    /// # Arguments
    ///
    /// * `o` - Read options handle
    /// * `val` - Non-zero to fill cache, zero to skip
    ///
    /// # Safety
    ///
    /// - `o` must be a valid read options pointer
    pub fn leveldb_readoptions_set_fill_cache(o: *mut leveldb_readoptions_t, val: c_uchar);

    /// Set the snapshot to read from
    ///
    /// # Arguments
    ///
    /// * `o` - Read options handle
    /// * `snapshot` - Snapshot to read from
    ///
    /// # Safety
    ///
    /// - Both pointers must be valid
    pub fn leveldb_readoptions_set_snapshot(
        o: *mut leveldb_readoptions_t,
        snapshot: *const leveldb_snapshot_t,
    );

    // =========================================================================
    // Write Options Operations
    // =========================================================================

    /// Create a new write options handle
    ///
    /// # Returns
    ///
    /// Pointer to a new write options handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_writeoptions_destroy()` on the returned pointer
    pub fn leveldb_writeoptions_create() -> *mut leveldb_writeoptions_t;

    /// Destroy a write options handle and release its resources
    ///
    /// # Arguments
    ///
    /// * `o` - Write options handle to destroy
    ///
    /// # Safety
    ///
    /// - `o` must be a valid write options pointer
    pub fn leveldb_writeoptions_destroy(o: *mut leveldb_writeoptions_t);

    /// Configure whether to sync writes to disk
    ///
    /// # Arguments
    ///
    /// * `o` - Write options handle
    /// * `val` - Non-zero to sync, zero for async writes
    ///
    /// # Safety
    ///
    /// - `o` must be a valid write options pointer
    pub fn leveldb_writeoptions_set_sync(o: *mut leveldb_writeoptions_t, val: c_uchar);

    // =========================================================================
    // Cache Operations
    // =========================================================================

    /// Create a new LRU cache
    ///
    /// # Arguments
    ///
    /// * `capacity` - Cache capacity in bytes
    ///
    /// # Returns
    ///
    /// Pointer to a new cache handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_cache_destroy()` on the returned pointer
    pub fn leveldb_cache_create_lru(capacity: size_t) -> *mut leveldb_cache_t;

    /// Destroy a cache and release its resources
    ///
    /// # Arguments
    ///
    /// * `cache` - Cache to destroy
    ///
    /// # Safety
    ///
    /// - `cache` must be a valid cache pointer
    pub fn leveldb_cache_destroy(cache: *mut leveldb_cache_t);

    // =========================================================================
    // Environment Operations
    // =========================================================================

    /// Create the default environment
    ///
    /// # Returns
    ///
    /// Pointer to a new environment handle
    ///
    /// # Safety
    ///
    /// - Caller must call `leveldb_env_destroy()` on the returned pointer
    pub fn leveldb_create_default_env() -> *mut leveldb_env_t;

    /// Destroy an environment and release its resources
    ///
    /// # Arguments
    ///
    /// * `env` - Environment to destroy
    ///
    /// # Safety
    ///
    /// - `env` must be a valid environment pointer
    pub fn leveldb_env_destroy(env: *mut leveldb_env_t);

    // =========================================================================
    // Utility Functions
    // =========================================================================

    /// Free memory allocated by LevelDB
    ///
    /// # Arguments
    ///
    /// * `ptr` - Pointer to memory to free
    ///
    /// # Safety
    ///
    /// - `ptr` must be a pointer returned from LevelDB functions like `leveldb_get()`
    /// - After this call, the pointer becomes invalid
    pub fn leveldb_free(ptr: *mut c_void);

    // =========================================================================
    // Version Information
    // =========================================================================

    /// Get the major version number of LevelDB
    ///
    /// # Returns
    ///
    /// Major version number
    pub fn leveldb_major_version() -> c_int;

    /// Get the minor version number of LevelDB
    ///
    /// # Returns
    ///
    /// Minor version number
    pub fn leveldb_minor_version() -> c_int;
}

#[cfg(test)]
mod tests;
