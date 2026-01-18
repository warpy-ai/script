//! Ahead-of-time (AOT) compilation for tscl
//!
//! This module provides AOT compilation to standalone executables.
//! Currently a scaffold for future implementation.
//!
//! Future features:
//! - Object file generation
//! - Static linking with runtime
//! - Platform-specific binary output
//! - Link-time optimization (LTO)

use super::{BackendConfig, BackendError};
use crate::ir::IrModule;
use std::path::Path;

/// AOT compilation target format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Object file (.o)
    Object,
    /// Static library (.a)
    StaticLib,
    /// Shared library (.so/.dylib/.dll)
    SharedLib,
    /// Executable
    Executable,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Executable
    }
}

/// AOT compilation options
#[derive(Debug, Clone)]
pub struct AotOptions {
    /// Output format
    pub format: OutputFormat,
    /// Target triple (e.g., "x86_64-apple-darwin")
    pub target: Option<String>,
    /// Enable link-time optimization
    pub lto: bool,
    /// Strip debug symbols
    pub strip: bool,
}

impl Default for AotOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Executable,
            target: None,
            lto: false,
            strip: false,
        }
    }
}

/// AOT compiler state
pub struct AotCompiler {
    config: BackendConfig,
    options: AotOptions,
}

impl AotCompiler {
    /// Create a new AOT compiler
    pub fn new(config: &BackendConfig) -> Self {
        Self {
            config: config.clone(),
            options: AotOptions::default(),
        }
    }

    /// Set AOT options
    pub fn with_options(mut self, options: AotOptions) -> Self {
        self.options = options;
        self
    }

    /// Compile an IR module to a file
    pub fn compile_to_file(
        &mut self,
        _module: &IrModule,
        _output: &Path,
    ) -> Result<(), BackendError> {
        // TODO: Implement AOT compilation
        //
        // Steps:
        // 1. Create Cranelift ObjectModule instead of JITModule
        // 2. Compile all functions
        // 3. Generate object file
        // 4. Link with runtime library
        // 5. Write output file

        Err(BackendError::AotError(
            "AOT compilation not yet implemented. Use JIT mode for now.".into(),
        ))
    }

    /// Compile an IR module to bytes (object file in memory)
    pub fn compile_to_bytes(&mut self, _module: &IrModule) -> Result<Vec<u8>, BackendError> {
        Err(BackendError::AotError(
            "AOT compilation not yet implemented".into(),
        ))
    }
}

/// Get the default target triple for the current platform
pub fn default_target() -> String {
    target_lexicon::Triple::host().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_target() {
        let target = default_target();
        assert!(!target.is_empty());
        // Should contain architecture
        assert!(
            target.contains("x86_64")
                || target.contains("aarch64")
                || target.contains("arm")
                || target.contains("i686")
        );
    }

    #[test]
    fn test_aot_options_default() {
        let opts = AotOptions::default();
        assert_eq!(opts.format, OutputFormat::Executable);
        assert!(!opts.lto);
        assert!(!opts.strip);
    }
}
