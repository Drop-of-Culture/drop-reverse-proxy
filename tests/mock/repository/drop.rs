use async_trait::async_trait;
use drop_reverse_proxy::repository::drop::Drop;
use drop_reverse_proxy::repository::Repo;
use drop_reverse_proxy::repository::RepositoryError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct DropRepoMock {
    map: Arc<RwLock<HashMap<i32, Drop>>>
}
impl DropRepoMock {
    pub fn new() -> Self {
        Self { map: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn map(&self) -> &Arc<RwLock<HashMap<i32, Drop>>> {
        &self.map
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
        );
        let save_result = drop_repo.save_or_update(&drop).await;
        assert!(save_result.is_ok());
        assert_eq!(drop_repo.get(0).await.unwrap(), drop);
    }
}