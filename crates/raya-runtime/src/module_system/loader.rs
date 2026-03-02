use crate::error::RuntimeError;

use super::resolver::ModuleKey;
use super::std_module_registry;

#[derive(Debug, Clone)]
pub struct LoadedModuleSource {
    pub key: ModuleKey,
    pub display_name: String,
    pub source: String,
}

#[derive(Debug, Default, Clone)]
pub struct ModuleLoaderV2;

impl ModuleLoaderV2 {
    pub fn load(&self, key: &ModuleKey) -> Result<LoadedModuleSource, RuntimeError> {
        match key {
            ModuleKey::File(path) => {
                let canonical = path.canonicalize().map_err(RuntimeError::Io)?;
                let source = std::fs::read_to_string(&canonical).map_err(RuntimeError::Io)?;
                Ok(LoadedModuleSource {
                    key: ModuleKey::File(canonical.clone()),
                    display_name: canonical.display().to_string(),
                    source,
                })
            }
            ModuleKey::Std(canonical) => {
                let source = std_module_registry().get(canonical).ok_or_else(|| {
                    RuntimeError::Dependency(format!(
                        "Standard module source not found for '{}'",
                        canonical
                    ))
                })?;
                Ok(LoadedModuleSource {
                    key: ModuleKey::Std(canonical.clone()),
                    display_name: format!("std:{}", canonical),
                    source: source.to_string(),
                })
            }
        }
    }
}
