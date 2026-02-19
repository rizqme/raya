//! Integration tests for module caching
//!
//! Tests the complete cache infrastructure including storage, retrieval, and metadata.

use raya_pm::{Cache, ModuleMetadata};

/// Generate unique test data to avoid cache pollution between test runs
fn unique_test_data(test_name: &str) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}-{}", test_name, timestamp).into_bytes()
}

#[test]
fn test_cache_store_and_retrieve() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("store_and_retrieve");

    // Store the module
    let hash = cache.store(&test_data).unwrap();

    // Verify it exists
    assert!(cache.exists(&hash));

    // Retrieve it
    let retrieved = cache.retrieve(&hash).unwrap();
    assert_eq!(retrieved, test_data);
}

#[test]
fn test_cache_exists() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("exists_test");

    // Compute hash
    let hash = {
        use sha2::{Digest, Sha256};
        let h = Sha256::digest(&test_data);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&h);
        arr
    };

    // Store it
    cache.store(&test_data).unwrap();

    // Now it exists
    assert!(cache.exists(&hash));
}

#[test]
fn test_checksum_verification() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("checksum_test");

    let hash = cache.store(&test_data).unwrap();

    // Retrieving should verify checksum
    let result = cache.retrieve(&hash);
    assert!(result.is_ok());
}

#[test]
fn test_duplicate_store_idempotent() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("duplicate_test");

    // Store the same data multiple times
    let hash1 = cache.store(&test_data).unwrap();
    let hash2 = cache.store(&test_data).unwrap();
    let hash3 = cache.store(&test_data).unwrap();

    // Should all return the same hash
    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);

    // Should only be stored once
    let retrieved = cache.retrieve(&hash1).unwrap();
    assert_eq!(retrieved, test_data);
}

#[test]
fn test_metadata_storage() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("metadata_storage");

    // Store module
    let hash = cache.store(&test_data).unwrap();

    // Create metadata
    let mut metadata =
        ModuleMetadata::new("test-module".into(), "1.0.0".into(), hex::encode(hash));
    metadata.add_dependency("dep1".into(), "^1.0.0".into());
    metadata.add_dependency("dep2".into(), "~2.1.0".into());

    // Store metadata
    cache.store_metadata(&hash, &metadata).unwrap();

    // Retrieve metadata
    let loaded = cache.load_metadata(&hash).unwrap();

    assert_eq!(loaded.name, "test-module");
    assert_eq!(loaded.version, "1.0.0");
    assert_eq!(loaded.dependencies.len(), 2);
    assert_eq!(loaded.dependencies.get("dep1"), Some(&"^1.0.0".to_string()));
}

#[test]
fn test_metadata_round_trip() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("metadata_roundtrip");

    let hash = cache.store(&test_data).unwrap();

    let metadata = ModuleMetadata::new(
        "round-trip-module".into(),
        "2.3.4".into(),
        hex::encode(hash),
    )
    .with_description("A test module for round-trip testing".into())
    .with_author("Test Author".into())
    .with_license("MIT".into());

    // Store and load
    cache.store_metadata(&hash, &metadata).unwrap();
    let loaded = cache.load_metadata(&hash).unwrap();

    // Verify all fields
    assert_eq!(loaded.name, metadata.name);
    assert_eq!(loaded.version, metadata.version);
    assert_eq!(loaded.checksum, metadata.checksum);
    assert_eq!(loaded.description, metadata.description);
    assert_eq!(loaded.author, metadata.author);
    assert_eq!(loaded.license, metadata.license);
}

#[test]
fn test_multiple_modules() {
    let cache = Cache::init().unwrap();

    let modules = vec![
        unique_test_data("module1"),
        unique_test_data("module2"),
        unique_test_data("module3"),
    ];

    let mut hashes = Vec::new();

    // Store multiple modules
    for module_data in &modules {
        let hash = cache.store(module_data).unwrap();
        hashes.push(hash);
    }

    // All hashes should be different
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[1], hashes[2]);
    assert_ne!(hashes[0], hashes[2]);

    // All should be retrievable
    for (i, hash) in hashes.iter().enumerate() {
        let retrieved = cache.retrieve(hash).unwrap();
        assert_eq!(retrieved, modules[i]);
    }
}

#[test]
fn test_large_module() {
    let cache = Cache::init().unwrap();

    // Create a large module (1 MB) with unique content
    let mut large_data = vec![0xAB; 1024 * 1024];
    // Make it unique by appending timestamp
    large_data.extend_from_slice(&unique_test_data("large_module"));

    // Store it
    let hash = cache.store(&large_data).unwrap();

    // Retrieve it
    let retrieved = cache.retrieve(&hash).unwrap();
    assert_eq!(retrieved.len(), large_data.len());
    assert_eq!(retrieved, large_data);
}

#[test]
fn test_module_not_found() {
    let cache = Cache::init().unwrap();

    // Try to retrieve a non-existent module
    let fake_hash = [0xFF; 32];
    let result = cache.retrieve(&fake_hash);

    assert!(result.is_err());
}

#[test]
fn test_cache_paths() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("cache_paths");

    let hash = cache.store(&test_data).unwrap();

    // Check paths
    let module_path = cache.module_path(&hash);
    let metadata_path = cache.metadata_path(&hash);

    assert!(module_path.exists());
    assert!(module_path.to_string_lossy().contains(&hex::encode(hash)));
    assert!(module_path.to_string_lossy().ends_with("module.ryb"));

    // Metadata path should be in the same directory
    assert_eq!(
        module_path.parent(),
        metadata_path.parent(),
        "Module and metadata should be in the same directory"
    );
}

#[test]
fn test_metadata_with_dependencies() {
    let cache = Cache::init().unwrap();
    let test_data = unique_test_data("complex_deps");

    let hash = cache.store(&test_data).unwrap();

    let mut metadata =
        ModuleMetadata::new("complex-module".into(), "3.2.1".into(), hex::encode(hash));

    // Add multiple dependencies
    metadata.add_dependency("logging".into(), "^1.0.0".into());
    metadata.add_dependency("http".into(), "~2.1.0".into());
    metadata.add_dependency("async".into(), ">=3.0.0".into());
    metadata.add_dependency("@org/utils".into(), "^1.5.0".into());

    cache.store_metadata(&hash, &metadata).unwrap();

    let loaded = cache.load_metadata(&hash).unwrap();

    assert_eq!(loaded.dependencies.len(), 4);
    assert!(loaded.dependencies.contains_key("logging"));
    assert!(loaded.dependencies.contains_key("http"));
    assert!(loaded.dependencies.contains_key("async"));
    assert!(loaded.dependencies.contains_key("@org/utils"));
}
