use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

#[derive(Clone)]
pub struct CfgFilesTracker(Arc<RwLock<HashMap<u64, String>>>);

impl CfgFilesTracker {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(HashMap::new())))
    }

    pub async fn add_file(&self, id: u64, name: String) {
        let mut files = self.0.write().await;
        files.insert(id, name);
    }

    pub async fn remove_file(&self, id: &u64) {
        let mut files = self.0.write().await;
        files.remove(id);
    }

    pub async fn get_file_name(&self, id: &u64) -> Option<String> {
        let files = self.0.read().await;
        files.get(id).cloned()
    }
}

impl Default for CfgFilesTracker {
    fn default() -> Self {
        Self::new()
    }
}
