//! # aria-skill-runtime
//!
//! Sandboxed WebAssembly skill executor for ARIA-X.
//!
//! Provides the [`WasmExecutor`] trait and [`ExtismBackend`] implementation
//! for running Wasm-based skills inside a strict sandbox with configurable
//! memory limits and no host filesystem access.

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from the Wasm skill runtime.
#[derive(Debug)]
pub enum RuntimeError {
    /// The Wasm module could not be loaded or instantiated.
    LoadError(String),
    /// Execution of the Wasm function failed.
    ExecutionError(String),
    /// A capability violation occurred (e.g. unauthorized host call).
    CapabilityViolation(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::LoadError(msg) => write!(f, "wasm load error: {}", msg),
            RuntimeError::ExecutionError(msg) => write!(f, "wasm execution error: {}", msg),
            RuntimeError::CapabilityViolation(msg) => {
                write!(f, "capability violation: {}", msg)
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

// ---------------------------------------------------------------------------
// WasmExecutor trait
// ---------------------------------------------------------------------------

/// Trait for executing WebAssembly skill modules.
pub trait WasmExecutor {
    /// Execute a function named `function_name` within the given Wasm module bytes.
    ///
    /// - `module`: Raw Wasm bytes (`.wasm` file contents)
    /// - `function_name`: Name of the exported function to call
    /// - `input`: String input to pass to the function
    ///
    /// Returns the string output from the function.
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError>;
}

// ---------------------------------------------------------------------------
// ExtismBackend
// ---------------------------------------------------------------------------

/// Configuration for the Extism-based Wasm executor.
#[derive(Debug, Clone)]
pub struct ExtismConfig {
    /// Maximum memory pages the Wasm module is allowed to use.
    /// Each page is 64KB. Default: 256 pages (16MB).
    pub max_memory_pages: Option<u32>,
    /// Whether WASI support is enabled. Default: false (strict sandbox).
    pub wasi_enabled: bool,
}

impl Default for ExtismConfig {
    fn default() -> Self {
        Self {
            max_memory_pages: Some(256),
            wasi_enabled: false,
        }
    }
}

/// Extism-based Wasm executor providing strict sandboxing.
///
/// No host functions are exposed. WASI is disabled by default.
/// Wasm modules can only process input → output via the Extism PDK interface.
pub struct ExtismBackend {
    config: ExtismConfig,
}

impl Default for ExtismBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtismBackend {
    /// Create a new Extism backend with default configuration.
    pub fn new() -> Self {
        Self {
            config: ExtismConfig::default(),
        }
    }

    /// Create a new Extism backend with custom configuration.
    pub fn with_config(config: ExtismConfig) -> Self {
        Self { config }
    }
}

impl WasmExecutor for ExtismBackend {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        // Build the manifest from raw bytes
        let wasm = extism::Wasm::data(module.to_vec());
        let mut manifest = extism::Manifest::new([wasm]);

        // Apply memory limits
        if let Some(max_pages) = self.config.max_memory_pages {
            manifest = manifest.with_memory_max(max_pages);
        }

        // Create the plugin — no host functions, WASI controlled by config
        let mut plugin = extism::Plugin::new(&manifest, [], self.config.wasi_enabled)
            .map_err(|e| RuntimeError::LoadError(format!("{}", e)))?;

        // Call the function
        let output: String = plugin
            .call::<&str, String>(function_name, input)
            .map_err(|e| {
                let msg = format!("{}", e);
                // Distinguish capability violations from general execution errors
                if msg.contains("not found") || msg.contains("unknown import") {
                    RuntimeError::CapabilityViolation(msg)
                } else {
                    RuntimeError::ExecutionError(msg)
                }
            })?;

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Path to the pre-built hello plugin Wasm fixture.
    fn hello_wasm_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-fixtures")
            .join("hello.wasm")
    }

    /// Load the hello plugin Wasm bytes.
    fn hello_wasm_bytes() -> Vec<u8> {
        std::fs::read(hello_wasm_path()).expect("hello.wasm fixture must exist")
    }

    // =====================================================================
    // Core execution tests
    // =====================================================================

    #[test]
    fn execute_hello_returns_hello() {
        let backend = ExtismBackend::new();
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "greet", "world");
        assert!(result.is_ok(), "execution failed: {:?}", result.err());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn execute_with_custom_config() {
        let config = ExtismConfig {
            max_memory_pages: Some(128),
            wasi_enabled: false,
        };
        let backend = ExtismBackend::with_config(config);
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "greet", "");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    // =====================================================================
    // Error handling tests
    // =====================================================================

    #[test]
    fn execute_missing_function_returns_error() {
        let backend = ExtismBackend::new();
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "nonexistent_function", "input");
        assert!(result.is_err(), "should fail for missing function");
        match result {
            Err(RuntimeError::ExecutionError(msg) | RuntimeError::CapabilityViolation(msg)) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("expected ExecutionError or CapabilityViolation"),
        }
    }

    #[test]
    fn execute_corrupted_bytes_returns_load_error() {
        let backend = ExtismBackend::new();
        let garbage = vec![0xFF, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
        let result = backend.execute(&garbage, "greet", "");
        assert!(result.is_err(), "corrupted bytes should fail to load");
        match result {
            Err(RuntimeError::LoadError(msg)) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected LoadError, got: {:?}", other),
        }
    }

    #[test]
    fn execute_empty_bytes_returns_load_error() {
        let backend = ExtismBackend::new();
        let result = backend.execute(&[], "greet", "");
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LoadError(_)) => {} // expected
            other => panic!("expected LoadError, got: {:?}", other),
        }
    }

    // =====================================================================
    // Config and error display tests
    // =====================================================================

    #[test]
    fn default_config_values() {
        let config = ExtismConfig::default();
        assert_eq!(config.max_memory_pages, Some(256));
        assert!(!config.wasi_enabled);
    }

    #[test]
    fn error_display() {
        let err = RuntimeError::LoadError("bad module".into());
        assert!(format!("{}", err).contains("wasm load error"));

        let err = RuntimeError::ExecutionError("trap".into());
        assert!(format!("{}", err).contains("wasm execution error"));

        let err = RuntimeError::CapabilityViolation("no fs".into());
        assert!(format!("{}", err).contains("capability violation"));
    }
}
