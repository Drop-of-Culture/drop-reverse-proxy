use async_trait::async_trait;
use drop_reverse_proxy::repository::drop::Drop;
use drop_reverse_proxy::repository::{Repo, RepoByToken};
use drop_reverse_proxy::repository::RepositoryError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct DropRepoMock {
    map: Arc<RwLock<HashMap<i32, Drop>>>,
    /// token id -> drop id
    map_by_token: Arc<RwLock<HashMap<Uuid, i32>>>,
}
impl DropRepoMock {
    pub fn new() -> Self {
        Self {
            map: Arc::new(RwLock::new(HashMap::new())),
            map_by_token: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn map(&self) -> &Arc<RwLock<HashMap<i32, Drop>>> {
        &self.map
    }

    pub fn map_by_token(&self) -> &Arc<RwLock<HashMap<Uuid, i32>>> {
        &self.map_by_token
    }
}

#[async_trait]
impl Repo<Drop> for DropRepoMock {
    async fn get(&self, id: i32) -> Result<Drop, RepositoryError> {
        match self.map().read().unwrap().get(&id) {
            Some(drop) => Ok(drop.clone()),
            None => Err(RepositoryError::EntityNotFound)
        }
    }

    async fn save_or_update(&self, entity: &Drop) -> Result<i32, RepositoryError> {
        self.map.write().unwrap().insert(entity.id(), entity.clone());
        Ok(entity.id())
    }
}

#[async_trait]
impl RepoByToken<Drop> for DropRepoMock {
    async fn get_by_token(&self, token_id: &Uuid) -> Result<Drop, RepositoryError> {
        let drop_id = *self.map_by_token.read().unwrap().get(token_id).ok_or(RepositoryError::EntityNotFound)?;
        self.get(drop_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn drop_repo_mock() {
        let drop_repo = DropRepoMock::new();
        assert_eq!(drop_repo.map().read().unwrap().len(), 0);
    }

    #[tokio::test]
    pub async fn drop_repo_mock_get_save() {
        let drop_repo = DropRepoMock::new();
        let drop = Drop::new(
            0,
            10,
            String::from("drop1"),
            String::from("drop1_dir"),
            0
        );
        let save_result = drop_repo.save_or_update(&drop).await;
        assert!(save_result.is_ok());
        assert_eq!(drop_repo.get(0).await.unwrap(), drop);
    }
}