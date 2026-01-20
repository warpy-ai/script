pub mod diagnostics;
pub mod loader;
pub mod resolver;

pub use diagnostics::{DependencyInfo, ModuleError, ModuleErrorKind, ModuleResult, SourceLocation};
pub use loader::{
    LoadedModule, ModuleCache, ModuleExport, ModuleImport, ModuleLoader, ModuleNamespace,
    ModuleValue, ParsedModule,
};
pub use resolver::{ImportAssertions, ModuleResolver, ResolvedModule};

use std::path::PathBuf;
use std::sync::Arc;

pub async fn load_module(entry: &str) -> ModuleResult<Arc<loader::LoadedModule>> {
    let mut loader = loader::ModuleLoader::new();
    let entry_path = PathBuf::from(entry);
    loader.load(&entry_path).await
}

pub fn load_module_sync(entry: &str) -> ModuleResult<Arc<loader::LoadedModule>> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| diagnostics::ModuleError::io_error(PathBuf::from(entry), e.to_string()))?;

    runtime.block_on(async {
        let mut loader = loader::ModuleLoader::new();
        let entry_path = PathBuf::from(entry);
        loader.load(&entry_path).await
    })
}
