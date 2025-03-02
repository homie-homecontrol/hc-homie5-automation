use std::{collections::HashMap, path::Path, sync::Arc};

use config_watcher::ConfigItemEvent;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct LuaModuleManager {
    // Maps file hash (filename hash) -> cleaned filename (basename without extension)
    file_names: Arc<RwLock<HashMap<u64, String>>>,

    // Maps cleaned filename -> file content
    file_contents: Arc<RwLock<HashMap<String, String>>>,
}

impl LuaModuleManager {
    pub fn new() -> Self {
        Self {
            file_names: Arc::new(RwLock::new(HashMap::new())),
            file_contents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn handle_event(&self, event: ConfigItemEvent<String>) {
        match event {
            ConfigItemEvent::NewDocument(filename_hash, filename) => {
                let cleaned_name = Self::extract_clean_filename(&filename);
                self.file_names.write().await.insert(filename_hash, cleaned_name);
            }

            ConfigItemEvent::New(item_hash, content) => {
                if let Some(cleaned_name) = self.file_names.read().await.get(&item_hash.filename_hash()).cloned() {
                    log::debug!("Adding lua file: {}: \n{}", cleaned_name, content);
                    self.file_contents.write().await.insert(cleaned_name, content);
                }
            }

            ConfigItemEvent::RemoveDocument(filename_hash) => {
                if let Some(cleaned_name) = self.file_names.write().await.remove(&filename_hash) {
                    self.file_contents.write().await.remove(&cleaned_name);
                }
            }

            _ => {}
        }
    }

    /// Gives read-only access to the file contents map, perfect for `setup_custom_loader`
    pub fn file_contents(&self) -> Arc<RwLock<HashMap<String, String>>> {
        Arc::clone(&self.file_contents)
    }

    /// Utility: Extract clean filename (basename, no `.lua` extension)
    fn extract_clean_filename(path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.strip_suffix(".lua").unwrap_or(name).to_string())
            .unwrap_or_else(|| path.to_string())
    }
}

impl Default for LuaModuleManager {
    fn default() -> Self {
        Self::new()
    }
}
