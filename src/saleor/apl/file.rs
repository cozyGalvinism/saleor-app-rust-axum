use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use super::{AplStore, AplId, AuthData};

pub struct FileAplStore;

#[async_trait]
impl AplStore for FileAplStore {
    async fn get(&self, apl_id: &AplId) -> Option<AuthData> {
        let file = tokio::fs::read_to_string(".saleor-app-auth.json").await.unwrap();
        let auth_data: AuthData = serde_json::from_str(&file).unwrap();

        Some(auth_data)
    }

    async fn set(&self, apl_id: &AplId, auth_data: AuthData) {
        let json = serde_json::to_string(&auth_data).unwrap();
        let mut file = tokio::fs::File::create(".saleor-app-auth.json").await.unwrap();
        file.write_all(json.as_bytes()).await.unwrap();
    }

    async fn remove(&self, apl_id: &AplId) {
        tokio::fs::remove_file(".saleor-app-auth.json").await.unwrap();
    }
}
