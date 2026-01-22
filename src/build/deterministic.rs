//! Deterministic build verification and utilities
//!
//! This module provides tools for verifying that builds are reproducible
//! and for comparing build artifacts across compilations.

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Version of the determinism verification protocol
pub const DETERMINISM_VERSION: u32 = 1;

/// Configuration for deterministic builds
#[derive(Debug, Clone)]
pub struct DeterminismConfig {
    /// Whether to sort symbols before emission
    pub sort_symbols: bool,
    /// Whether to use fixed hash seeds
    pub fixed_hash_seed: bool,
    /// Whether to strip timestamps from output
    pub strip_timestamps: bool,
    /// Whether to normalize paths in output
    pub normalize_paths: bool,
}

impl Default for DeterminismConfig {
    fn default() -> Self {
        Self {
            sort_symbols: true,
            fixed_hash_seed: true,
            strip_timestamps: true,
            normalize_paths: true,
        }
    }
}

/// Result of determinism verification
#[derive(Debug)]
pub struct VerificationResult {
    /// Whether the two artifacts are identical
    pub is_deterministic: bool,
    /// SHA256 hash of first artifact
    pub hash_a: String,
    /// SHA256 hash of second artifact
    pub hash_b: String,
    /// Size of first artifact in bytes
    pub size_a: usize,
    /// Size of second artifact in bytes
    pub size_b: usize,
    /// List of differences found (if any)
    pub differences: Vec<DifferenceReport>,
}

/// Report of a single difference between two artifacts
#[derive(Debug)]
pub struct DifferenceReport {
    /// Byte offset where difference was found
    pub offset: usize,
    /// Byte value in first artifact
    pub byte_a: u8,
    /// Byte value in second artifact
    pub byte_b: u8,
    /// Section name if known (e.g., ".text", ".data")
    pub section: Option<String>,
}

/// Compute SHA256 hash of a file
pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute SHA256 hash of bytes
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Compare two build artifacts for bit-for-bit equality
pub fn compare_artifacts(path_a: &Path, path_b: &Path) -> io::Result<VerificationResult> {
    let data_a = fs::read(path_a)?;
    let data_b = fs::read(path_b)?;

    let hash_a = hash_bytes(&data_a);
    let hash_b = hash_bytes(&data_b);

    let mut differences = Vec::new();

    // Find first N differences
    const MAX_DIFFERENCES: usize = 10;
    let min_len = data_a.len().min(data_b.len());

    for i in 0..min_len {
        if data_a[i] != data_b[i] {
            differences.push(DifferenceReport {
                offset: i,
                byte_a: data_a[i],
                byte_b: data_b[i],
                section: None,
            });
            if differences.len() >= MAX_DIFFERENCES {
                break;
            }
        }
    }

    // Report size difference
    if data_a.len() != data_b.len() && differences.len() < MAX_DIFFERENCES {
        differences.push(DifferenceReport {
            offset: min_len,
            byte_a: 0,
            byte_b: 0,
            section: Some(format!(
                "Size difference: {} vs {}",
                data_a.len(),
                data_b.len()
            )),
        });
    }

    Ok(VerificationResult {
        is_deterministic: hash_a == hash_b,
        hash_a,
        hash_b,
        size_a: data_a.len(),
        size_b: data_b.len(),
        differences,
    })
}

/// Verify that two consecutive builds produce identical output
///
/// This function compiles the same source twice using the provided build function
/// and verifies that the outputs are identical.
pub fn verify_determinism<F, E>(
    source_path: &Path,
    temp_dir: &Path,
    build_fn: F,
) -> Result<VerificationResult, E>
where
    F: Fn(&Path, &Path) -> Result<(), E>,
{
    let output_a = temp_dir.join("determinism_test_a");
    let output_b = temp_dir.join("determinism_test_b");

    // Build twice
    build_fn(source_path, &output_a)?;
    build_fn(source_path, &output_b)?;

    // Compare outputs
    compare_artifacts(&output_a, &output_b)
        .map_err(|e| panic!("Failed to compare artifacts: {}", e))
}

/// Get deterministic timestamp for build metadata
///
/// Returns SOURCE_DATE_EPOCH if set (for reproducible builds),
/// otherwise returns 0 (epoch).
pub fn deterministic_timestamp() -> u64 {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Normalize environment for deterministic builds
///
/// This function saves the current environment and sets up
/// a normalized environment for reproducible compilation.
pub struct NormalizedEnv {
    original_env: std::collections::HashMap<String, String>,
}

impl NormalizedEnv {
    /// Create a normalized build environment
    ///
    /// # Safety
    /// This function modifies environment variables, which is inherently
    /// unsafe in multi-threaded contexts. Use only in single-threaded
    /// build processes.
    pub fn new() -> Self {
        let mut original_env = std::collections::HashMap::new();

        // Save and normalize relevant env vars
        for key in &["LANG", "LC_ALL", "TZ", "SOURCE_DATE_EPOCH"] {
            if let Ok(val) = std::env::var(key) {
                original_env.insert(key.to_string(), val);
            }
        }

        // SAFETY: Environment variable modification is only safe in single-threaded
        // contexts. Build processes are typically single-threaded.
        unsafe {
            // Set normalized values
            std::env::set_var("LANG", "C");
            std::env::set_var("LC_ALL", "C");
            std::env::set_var("TZ", "UTC");

            // Set SOURCE_DATE_EPOCH if not already set (for reproducible timestamps)
            if std::env::var("SOURCE_DATE_EPOCH").is_err() {
                std::env::set_var("SOURCE_DATE_EPOCH", "0");
            }
        }

        Self { original_env }
    }
}

impl Default for NormalizedEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NormalizedEnv {
    fn drop(&mut self) {
        // SAFETY: Environment variable modification is only safe in single-threaded
        // contexts. Build processes are typically single-threaded.
        unsafe {
            // Restore original environment
            for (key, val) in &self.original_env {
                std::env::set_var(key, val);
            }

            // Remove vars that weren't originally set
            for key in &["LANG", "LC_ALL", "TZ", "SOURCE_DATE_EPOCH"] {
                if !self.original_env.contains_key(*key) {
                    std::env::remove_var(key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// Helper to create a unique temp directory for tests
    fn create_test_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tscl_test_{}", test_name));
        let _ = fs::remove_dir_all(&dir); // Clean up if exists
        fs::create_dir_all(&dir).expect("Failed to create test dir");
        dir
    }

    /// Helper to clean up test directory
    fn cleanup_test_dir(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_hash_bytes() {
        let data = b"hello world";
        let hash = hash_bytes(data);
        // SHA256 of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hash_file() {
        let dir = create_test_dir("hash_file");
        let path = dir.join("test.txt");

        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"hello world").unwrap();
        drop(file);

        let hash = hash_file(&path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_compare_identical_artifacts() {
        let dir = create_test_dir("compare_identical");
        let path_a = dir.join("a.bin");
        let path_b = dir.join("b.bin");

        fs::write(&path_a, b"identical content").unwrap();
        fs::write(&path_b, b"identical content").unwrap();

        let result = compare_artifacts(&path_a, &path_b).unwrap();
        assert!(result.is_deterministic);
        assert_eq!(result.hash_a, result.hash_b);
        assert!(result.differences.is_empty());

        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_compare_different_artifacts() {
        let dir = create_test_dir("compare_different");
        let path_a = dir.join("a.bin");
        let path_b = dir.join("b.bin");

        fs::write(&path_a, b"content A").unwrap();
        fs::write(&path_b, b"content B").unwrap();

        let result = compare_artifacts(&path_a, &path_b).unwrap();
        assert!(!result.is_deterministic);
        assert_ne!(result.hash_a, result.hash_b);
        assert!(!result.differences.is_empty());

        cleanup_test_dir(&dir);
    }

    #[test]
    fn test_deterministic_timestamp() {
        // SAFETY: Environment variable tests should run in single-threaded context
        unsafe {
            // Without SOURCE_DATE_EPOCH set
            std::env::remove_var("SOURCE_DATE_EPOCH");
            assert_eq!(deterministic_timestamp(), 0);

            // With SOURCE_DATE_EPOCH set
            std::env::set_var("SOURCE_DATE_EPOCH", "1234567890");
            assert_eq!(deterministic_timestamp(), 1234567890);

            // Cleanup
            std::env::remove_var("SOURCE_DATE_EPOCH");
        }
    }

    #[test]
    fn test_normalized_env() {
        let _env = NormalizedEnv::new();
        assert_eq!(std::env::var("LANG").unwrap(), "C");
        assert_eq!(std::env::var("LC_ALL").unwrap(), "C");
        assert_eq!(std::env::var("TZ").unwrap(), "UTC");
        // NormalizedEnv is dropped here, restoring original env
    }

    #[test]
    fn test_determinism_config_default() {
        let config = DeterminismConfig::default();
        assert!(config.sort_symbols);
        assert!(config.fixed_hash_seed);
        assert!(config.strip_timestamps);
        assert!(config.normalize_paths);
    }
}
