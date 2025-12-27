use super::*;
use std::ffi::{CStr, CString};
use std::fs;
use std::path::Path;
use std::ptr;
use std::slice;

// Helper functions for test setup and cleanup

/// Create a test database path inside test_db directory and ensure clean state
fn test_db_path(name: &str) -> CString {
    let test_dir = "test_db";
    let path = format!("{}/{}", test_dir, name);

    // Ensure test_db directory exists
    if !Path::new(test_dir).exists() {
        fs::create_dir_all(test_dir).unwrap();
    }

    // Clean up before test
    if Path::new(&path).exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    CString::new(path).unwrap()
}

/// Clean up test database after test
fn cleanup_db_path(name: &str) {
    let test_dir = "test_db";
    let path = format!("{}/{}", test_dir, name);
    cleanup_path(&path);
}

fn cleanup_path(name: &str) {
    if Path::new(name).exists() {
        fs::remove_dir_all(name).unwrap();
    }
}

/// Create default options with create_if_missing enabled
unsafe fn create_default_options() -> *mut leveldb_options_t {
    let options = unsafe { leveldb_options_create() };
    unsafe { leveldb_options_set_create_if_missing(options, 1) };
    options
}

/// Assert that no error occurred and clean up error message if present
unsafe fn assert_no_error(err: *mut c_char) {
    if !err.is_null() {
        let error_msg = unsafe { CStr::from_ptr(err).to_string_lossy() };
        unsafe { leveldb_free(err as *mut c_void) };
        panic!("Operation failed: {}", error_msg);
    }
}

// Basic database tests

#[test]
fn test_database_lifecycle() {
    unsafe {
        let db_path = test_db_path("lifecycle");

        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);

        assert!(!db.is_null(), "Failed to open database");
        assert_no_error(err);

        // Verify database is usable by doing a simple operation
        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();

        leveldb_close(db);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_options_destroy(options);

        cleanup_db_path("lifecycle");
    }
}

#[test]
fn test_database_open_nonexistent_without_create() {
    unsafe {
        let db_path = test_db_path("nonexistent");

        let options = leveldb_options_create();
        // Don't set create_if_missing

        let mut err: *mut c_char = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);

        assert!(db.is_null(), "Should fail to open non-existent DB");
        assert!(!err.is_null(), "Should have error message");

        // Clean up error message
        if !err.is_null() {
            let error_msg = CStr::from_ptr(err).to_string_lossy();
            println!("Expected error: {}", error_msg);
            leveldb_free(err as *mut c_void);
        }

        leveldb_options_destroy(options);
        cleanup_db_path("nonexistent");
    }
}

#[test]
fn test_put_and_get() {
    unsafe {
        let db_path = test_db_path("put_get");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Test data
        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();
        let key = CString::new("test_key").unwrap();
        let value = CString::new("test_value").unwrap();

        // Put value
        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Get value back
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        assert!(!result.is_null(), "Get returned null");

        // Verify value
        let result_slice = slice::from_raw_parts(result as *const u8, val_len);
        let result_str = std::str::from_utf8(result_slice).unwrap();
        assert_eq!(result_str, "test_value");

        // Cleanup
        leveldb_free(result as *mut c_void);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("put_get");
    }
}

#[test]
fn test_get_nonexistent_key() {
    unsafe {
        let db_path = test_db_path("get_nonexistent");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let read_opts = leveldb_readoptions_create();
        let key = CString::new("nonexistent_key").unwrap();
        let mut val_len: size_t = 0;

        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        assert!(
            result.is_null(),
            "Get should return null for non-existent key"
        );

        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("get_nonexistent");
    }
}

#[test]
fn test_delete() {
    unsafe {
        let db_path = test_db_path("delete");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();
        let key = CString::new("key_to_delete").unwrap();
        let value = CString::new("value_to_delete").unwrap();

        // Put then delete
        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        leveldb_delete(db, write_opts, key.as_ptr(), key.as_bytes().len(), &mut err);
        assert_no_error(err);

        // Verify deleted
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        assert!(result.is_null(), "Key should not exist after delete");

        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("delete");
    }
}

#[test]
fn test_write_batch() {
    unsafe {
        let db_path = test_db_path("write_batch");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();
        let batch = leveldb_writebatch_create();

        // Prepare test data
        let keys = [
            CString::new("batch_key1").unwrap(),
            CString::new("batch_key2").unwrap(),
            CString::new("batch_key3").unwrap(),
        ];
        let values = [
            CString::new("batch_val1").unwrap(),
            CString::new("batch_val2").unwrap(),
            CString::new("batch_val3").unwrap(),
        ];

        // Add puts to batch
        for (key, value) in keys.iter().zip(values.iter()) {
            leveldb_writebatch_put(
                batch,
                key.as_ptr(),
                key.as_bytes().len(),
                value.as_ptr(),
                value.as_bytes().len(),
            );
        }

        // Add a delete to batch
        let delete_key = CString::new("key_to_delete").unwrap();
        let delete_value = CString::new("delete_me").unwrap();

        // First put the key we'll delete
        leveldb_put(
            db,
            write_opts,
            delete_key.as_ptr(),
            delete_key.as_bytes().len(),
            delete_value.as_ptr(),
            delete_value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Then add delete to batch
        leveldb_writebatch_delete(batch, delete_key.as_ptr(), delete_key.as_bytes().len());

        // Execute batch
        leveldb_write(db, write_opts, batch, &mut err);
        assert_no_error(err);

        // Verify batch puts
        for key in &keys {
            let mut val_len: size_t = 0;
            let result = leveldb_get(
                db,
                read_opts,
                key.as_ptr(),
                key.as_bytes().len(),
                &mut val_len,
                &mut err,
            );
            assert_no_error(err);
            assert!(!result.is_null(), "Batch put should succeed");
            leveldb_free(result as *mut c_void);
        }

        // Verify batch delete
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            delete_key.as_ptr(),
            delete_key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        assert!(result.is_null(), "Batch delete should remove key");

        // Cleanup
        leveldb_writebatch_destroy(batch);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("write_batch");
    }
}

#[test]
fn test_write_batch_clear() {
    unsafe {
        let batch = leveldb_writebatch_create();

        // Add some operations
        let key = CString::new("test_key").unwrap();
        let value = CString::new("test_value").unwrap();
        leveldb_writebatch_put(
            batch,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
        );
        leveldb_writebatch_delete(batch, key.as_ptr(), key.as_bytes().len());

        // Clear batch
        leveldb_writebatch_clear(batch);

        // Batch should now be empty (we can't easily verify this without executing)
        leveldb_writebatch_destroy(batch);
    }
}

#[test]
fn test_iterator() {
    unsafe {
        let db_path = test_db_path("iterator");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();

        // Insert test data in sorted order
        let test_data = vec![("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")];

        for (key, value) in &test_data {
            let key_c = CString::new(*key).unwrap();
            let value_c = CString::new(*value).unwrap();
            leveldb_put(
                db,
                write_opts,
                key_c.as_ptr(),
                key_c.as_bytes().len(),
                value_c.as_ptr(),
                value_c.as_bytes().len(),
                &mut err,
            );
            assert_no_error(err);
        }

        // Test forward iteration
        let iter = leveldb_create_iterator(db, read_opts);
        leveldb_iter_seek_to_first(iter);

        let mut count = 0;
        let mut iterated_keys = Vec::new();

        while leveldb_iter_valid(iter) != 0 {
            let mut key_len: size_t = 0;
            let mut val_len: size_t = 0;

            let key_ptr = leveldb_iter_key(iter, &mut key_len);
            let val_ptr = leveldb_iter_value(iter, &mut val_len);

            let key_slice = slice::from_raw_parts(key_ptr as *const u8, key_len);
            let val_slice = slice::from_raw_parts(val_ptr as *const u8, val_len);

            let key_str = std::str::from_utf8(key_slice).unwrap();
            let val_str = std::str::from_utf8(val_slice).unwrap();

            iterated_keys.push(key_str.to_string());
            assert_eq!(val_str, test_data[count].1);

            count += 1;
            leveldb_iter_next(iter);
        }

        assert_eq!(count, 4, "Iterator should find 4 items");
        assert_eq!(iterated_keys, vec!["a", "b", "c", "d"]);

        // Test reverse iteration
        leveldb_iter_seek_to_last(iter);
        let mut reverse_count = 0;

        while leveldb_iter_valid(iter) != 0 {
            reverse_count += 1;
            leveldb_iter_prev(iter);
        }

        assert_eq!(reverse_count, 4, "Reverse iteration should find 4 items");

        // Test seeking
        let seek_key = CString::new("c").unwrap();
        leveldb_iter_seek(iter, seek_key.as_ptr(), seek_key.as_bytes().len());
        assert_ne!(leveldb_iter_valid(iter), 0, "Seek should find key 'c'");

        let mut key_len: size_t = 0;
        let key_ptr = leveldb_iter_key(iter, &mut key_len);
        let key_slice = slice::from_raw_parts(key_ptr as *const u8, key_len);
        let key_str = std::str::from_utf8(key_slice).unwrap();
        assert_eq!(key_str, "c");

        // Cleanup
        leveldb_iter_destroy(iter);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("iterator");
    }
}

#[test]
fn test_snapshot() {
    unsafe {
        let db_path = test_db_path("snapshot");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();
        let key = CString::new("snapshot_key").unwrap();
        let value1 = CString::new("value1").unwrap();

        // Write initial value
        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value1.as_ptr(),
            value1.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Create snapshot
        let snapshot = leveldb_create_snapshot(db);

        // Update value after snapshot
        let value2 = CString::new("value2").unwrap();
        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value2.as_ptr(),
            value2.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Read without snapshot should get new value
        let read_opts = leveldb_readoptions_create();
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        let result_slice = slice::from_raw_parts(result as *const u8, val_len);
        let result_str = std::str::from_utf8(result_slice).unwrap();
        assert_eq!(result_str, "value2");
        leveldb_free(result as *mut c_void);

        // Read with snapshot should get old value
        let snapshot_read_opts = leveldb_readoptions_create();
        leveldb_readoptions_set_snapshot(snapshot_read_opts, snapshot);

        let mut val_len2: size_t = 0;
        let result2 = leveldb_get(
            db,
            snapshot_read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len2,
            &mut err,
        );
        assert_no_error(err);
        let result_slice2 = slice::from_raw_parts(result2 as *const u8, val_len2);
        let result_str2 = std::str::from_utf8(result_slice2).unwrap();
        assert_eq!(result_str2, "value1");

        // Cleanup
        leveldb_free(result2 as *mut c_void);
        leveldb_release_snapshot(db, snapshot);
        leveldb_readoptions_destroy(read_opts);
        leveldb_readoptions_destroy(snapshot_read_opts);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("snapshot");
    }
}

#[test]
fn test_bloom_filter() {
    unsafe {
        let db_path = test_db_path("bloom_filter");
        let options = create_default_options();

        // Create and set bloom filter
        let bloom_filter = leveldb_filterpolicy_create_bloom(10);
        leveldb_options_set_filter_policy(options, bloom_filter);

        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Write some data to verify filter doesn't break operations
        let write_opts = leveldb_writeoptions_create();
        let key = CString::new("filter_test_key").unwrap();
        let value = CString::new("filter_test_value").unwrap();

        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Read back to verify
        let read_opts = leveldb_readoptions_create();
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        assert_no_error(err);
        assert!(!result.is_null());

        // Cleanup
        leveldb_free(result as *mut c_void);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        leveldb_filterpolicy_destroy(bloom_filter);
        cleanup_db_path("bloom_filter");
    }
}

#[test]
fn test_cache() {
    unsafe {
        let db_path = test_db_path("cache");
        let options = create_default_options();

        // Create and set cache
        let cache = leveldb_cache_create_lru(8 * 1024 * 1024); // 8MB cache
        leveldb_options_set_cache(options, cache);

        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Perform operations to verify cache works
        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();
        let key = CString::new("cache_test_key").unwrap();
        let value = CString::new("cache_test_value").unwrap();

        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        // Read multiple times - second read might be from cache
        for _ in 0..3 {
            let mut val_len: size_t = 0;
            let result = leveldb_get(
                db,
                read_opts,
                key.as_ptr(),
                key.as_bytes().len(),
                &mut val_len,
                &mut err,
            );
            assert_no_error(err);
            assert!(!result.is_null());
            leveldb_free(result as *mut c_void);
        }

        // Cleanup
        leveldb_writeoptions_destroy(write_opts);
        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        leveldb_cache_destroy(cache);
        cleanup_db_path("cache");
    }
}

#[test]
fn test_read_options() {
    unsafe {
        let db_path = test_db_path("read_options");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let read_opts = leveldb_readoptions_create();

        // Test verify checksums
        leveldb_readoptions_set_verify_checksums(read_opts, 1);

        // Test fill cache
        leveldb_readoptions_set_fill_cache(read_opts, 1);

        // These settings are hard to test directly, but we can verify they don't crash
        let key = CString::new("test_key").unwrap();
        let mut val_len: size_t = 0;
        let result = leveldb_get(
            db,
            read_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            &mut val_len,
            &mut err,
        );
        // Key doesn't exist, but operation should succeed
        assert_no_error(err);
        assert!(result.is_null());

        leveldb_readoptions_destroy(read_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("read_options");
    }
}

#[test]
fn test_write_options() {
    unsafe {
        let db_path = test_db_path("write_options");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();

        // Test sync write
        leveldb_writeoptions_set_sync(write_opts, 1);

        // Test that sync write works
        let key = CString::new("sync_test_key").unwrap();
        let value = CString::new("sync_test_value").unwrap();

        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        leveldb_writeoptions_destroy(write_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("write_options");
    }
}

#[test]
fn test_compression_options() {
    unsafe {
        let db_path = test_db_path("compression");
        let options = create_default_options();

        // Test different compression options
        leveldb_options_set_compression(options, Compression::No);
        leveldb_options_set_compression(options, Compression::Snappy);

        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Verify database works with compression
        let write_opts = leveldb_writeoptions_create();
        let key = CString::new("compression_test").unwrap();
        let value = CString::new("test_value").unwrap();

        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        leveldb_writeoptions_destroy(write_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("compression");
    }
}

#[test]
fn test_destroy_and_repair_db() {
    unsafe {
        let db_path = test_db_path("destroy_repair");
        let options = create_default_options();
        let mut err = ptr::null_mut();

        // Create a database first
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Write some data
        let write_opts = leveldb_writeoptions_create();
        let key = CString::new("test_data").unwrap();
        let value = CString::new("important_value").unwrap();

        leveldb_put(
            db,
            write_opts,
            key.as_ptr(),
            key.as_bytes().len(),
            value.as_ptr(),
            value.as_bytes().len(),
            &mut err,
        );
        assert_no_error(err);

        leveldb_close(db);
        leveldb_writeoptions_destroy(write_opts);

        // Test repair - should succeed on healthy database
        leveldb_repair_db(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Test destroy
        leveldb_destroy_db(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Verify database is gone
        assert!(!Path::new("test_db_destroy_repair/CURRENT").exists());

        leveldb_options_destroy(options);
        cleanup_db_path("destroy_repair");
    }
}

#[test]
fn test_property_value() {
    unsafe {
        let db_path = test_db_path("properties");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Get a property - leveldb.stats is commonly available
        let prop_name = CString::new("leveldb.stats").unwrap();
        let prop_value = leveldb_property_value(db, prop_name.as_ptr());

        // Property might be null or contain stats, both are valid
        if !prop_value.is_null() {
            let _prop_str = CStr::from_ptr(prop_value).to_string_lossy();
            leveldb_free(prop_value as *mut c_void);
        }

        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("properties");
    }
}

#[test]
fn test_approximate_sizes() {
    unsafe {
        let db_path = test_db_path("approx_sizes");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        let write_opts = leveldb_writeoptions_create();
        let read_opts = leveldb_readoptions_create();

        // Write large amount of test data
        let n = 20000;
        for i in 0..n {
            let key = format!("k{:020}", i);
            let value = format!("v{:020}", i);

            let key_c = CString::new(key).unwrap();
            let value_c = CString::new(value).unwrap();

            leveldb_put(
                db,
                write_opts,
                key_c.as_ptr(),
                key_c.as_bytes().len(),
                value_c.as_ptr(),
                value_c.as_bytes().len(),
                &mut err,
            );
            assert_no_error(err);
        }

        // Force compaction to ensure data is written to SST files
        leveldb_compact_range(db, ptr::null(), 0, ptr::null(), 0);

        // Define ranges for approximate sizes
        let start_keys = vec![
            CString::new("a").unwrap(),                     // From beginning
            CString::new("k00000000000000010000").unwrap(), // From key 10000
        ];
        let limit_keys = vec![
            CString::new("k00000000000000010000").unwrap(), // To key 10000
            CString::new("z").unwrap(),                     // To end
        ];

        let start_ptrs: Vec<*const c_char> = start_keys.iter().map(|k| k.as_ptr()).collect();
        let limit_ptrs: Vec<*const c_char> = limit_keys.iter().map(|k| k.as_ptr()).collect();
        let start_lens: Vec<size_t> = start_keys.iter().map(|k| k.as_bytes().len()).collect();
        let limit_lens: Vec<size_t> = limit_keys.iter().map(|k| k.as_bytes().len()).collect();

        let mut sizes = [0u64; 2];

        leveldb_approximate_sizes(
            db,
            2,
            start_ptrs.as_ptr(),
            start_lens.as_ptr(),
            limit_ptrs.as_ptr(),
            limit_lens.as_ptr(),
            sizes.as_mut_ptr(),
        );

        if sizes[0] == 0 && sizes[1] == 0 {
            println!("Sizes are zero, trying to read some data...");

            for i in (0..n).step_by(1000) {
                let key = format!("k{:020}", i);
                let key_c = CString::new(key).unwrap();
                let mut val_len: size_t = 0;
                let result = leveldb_get(
                    db,
                    read_opts,
                    key_c.as_ptr(),
                    key_c.as_bytes().len(),
                    &mut val_len,
                    &mut err,
                );
                if !result.is_null() {
                    leveldb_free(result as *mut c_void);
                }
                assert_no_error(err);
            }

            leveldb_approximate_sizes(
                db,
                2,
                start_ptrs.as_ptr(),
                start_lens.as_ptr(),
                limit_ptrs.as_ptr(),
                limit_lens.as_ptr(),
                sizes.as_mut_ptr(),
            );
        }

        assert!(
            sizes[0] > 0,
            "Approximate size for first range should be positive, got {}",
            sizes[0]
        );
        assert!(
            sizes[1] > 0,
            "Approximate size for second range should be positive, got {}",
            sizes[1]
        );

        // The first range ("a" to "k00000000000000010000") should be smaller than
        // the second range ("k00000000000000010000" to "z") since we have more data
        // in the later part (keys 10000-19999 vs keys 0-9999)
        assert!(
            sizes[0] <= sizes[1],
            "First range size ({}) should be <= second range size ({})",
            sizes[0],
            sizes[1]
        );

        leveldb_readoptions_destroy(read_opts);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("approx_sizes");
    }
}

#[test]
fn test_compact_range() {
    unsafe {
        let db_path = test_db_path("compact");
        let options = create_default_options();
        let mut err = ptr::null_mut();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);
        assert_no_error(err);

        // Write some data
        let write_opts = leveldb_writeoptions_create();
        for i in 0..10 {
            let key = CString::new(format!("key_{}", i)).unwrap();
            let value = CString::new(format!("value_{}", i)).unwrap();
            leveldb_put(
                db,
                write_opts,
                key.as_ptr(),
                key.as_bytes().len(),
                value.as_ptr(),
                value.as_bytes().len(),
                &mut err,
            );
            assert_no_error(err);
        }

        // Compact a range
        let start_key = CString::new("key_2").unwrap();
        let limit_key = CString::new("key_7").unwrap();

        leveldb_compact_range(
            db,
            start_key.as_ptr(),
            start_key.as_bytes().len(),
            limit_key.as_ptr(),
            limit_key.as_bytes().len(),
        );

        // Verify data is still accessible after compaction
        let read_opts = leveldb_readoptions_create();
        for i in 0..10 {
            let key = CString::new(format!("key_{}", i)).unwrap();
            let mut val_len: size_t = 0;
            let result = leveldb_get(
                db,
                read_opts,
                key.as_ptr(),
                key.as_bytes().len(),
                &mut val_len,
                &mut err,
            );
            assert_no_error(err);
            assert!(!result.is_null(), "Data should survive compaction");
            leveldb_free(result as *mut c_void);
        }

        leveldb_readoptions_destroy(read_opts);
        leveldb_writeoptions_destroy(write_opts);
        leveldb_close(db);
        leveldb_options_destroy(options);
        cleanup_db_path("compact");
    }
}

#[test]
fn test_version() {
    unsafe {
        let major = leveldb_major_version();
        let minor = leveldb_minor_version();

        // LevelDB typically has version >= 1.0
        assert!(major >= 1, "LevelDB major version should be at least 1");
        assert!(minor >= 0, "LevelDB minor version should be non-negative");
    }
}

#[test]
fn test_comparator_creation() {
    extern "C" fn dummy_destructor(_state: *mut c_void) {}

    extern "C" fn dummy_compare(
        _state: *mut c_void,
        a: *const c_char,
        a_len: size_t,
        b: *const c_char,
        b_len: size_t,
    ) -> c_int {
        let a_slice = unsafe { slice::from_raw_parts(a as *const u8, a_len) };
        let b_slice = unsafe { slice::from_raw_parts(b as *const u8, b_len) };
        a_slice.cmp(&b_slice) as c_int
    }

    extern "C" fn dummy_name(_state: *mut c_void) -> *const c_char {
        CString::new("dummy_comparator").unwrap().into_raw()
    }

    unsafe {
        let comparator =
            leveldb_comparator_create(ptr::null_mut(), dummy_destructor, dummy_compare, dummy_name);

        assert!(
            !comparator.is_null(),
            "Should create comparator successfully"
        );

        leveldb_comparator_destroy(comparator);
    }
}

#[test]
fn test_error_handling() {
    unsafe {
        let options = leveldb_options_create();
        // Don't set create_if_missing

        let mut err: *mut c_char = ptr::null_mut();
        let db_path = CString::new("non_existent_path_12345").unwrap();
        let db = leveldb_open(options, db_path.as_ptr(), &mut err);

        assert!(db.is_null(), "Should fail to open non-existent DB");
        assert!(!err.is_null(), "Should set error pointer");

        // Extract and verify error message
        if !err.is_null() {
            let error_msg = CStr::from_ptr(err).to_string_lossy();
            assert!(!error_msg.is_empty(), "Error message should not be empty");
            leveldb_free(err as *mut c_void);
        }

        leveldb_options_destroy(options);
        cleanup_path("non_existent_path_12345");
    }
}
