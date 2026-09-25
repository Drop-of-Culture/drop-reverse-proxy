use async_trait::async_trait;
use drop_reverse_proxy::repository::redirect::Redirect;
use drop_reverse_proxy::repository::Repo;
use drop_reverse_proxy::repository::RepositoryError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct RedirectRepoMock {
    map: Arc<RwLock<HashMap<i32, Redirect>>>
}
impl RedirectRepoMock {
    pub fn new() -> Self {
        Self { map: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn map(&self) -> &Arc<RwLock<HashMap<i32, Redirect>>> {
        &self.map
    }

    pub async fn get_by_drop_id(&self, drop_id: i32) -> Result<Redirect, RepositoryError> {
        match self.map().read().unwrap().values().find(|redirect| redirect.drop_id == drop_id) {
            Some(redirect) => Ok(redirect.clone()),
            None => Err(RepositoryError::EntityNotFound)
        }
    }
}

#[async_trait]
impl Repo<Redirect> for RedirectRepoMock {
    async fn get(&self, id: i32) -> Result<Redirect, RepositoryError> {
        match self.map().read().unwrap().get(&id) {
            Some(redirect) => Ok(redirect.clone()),
            None => Err(RepositoryError::EntityNotFound)
        }
    }

    async fn save_or_update(&self, entity: &Redirect) -> Result<i32, RepositoryError> {
        self.map.write().unwrap().insert(entity.id(), entity.clone());
        Ok(entity.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    pub fn redirect_repo_mock() {
        let redirect_repo = RedirectRepoMock::new();
        assert_eq!(redirect_repo.map().read().unwrap().len(), 0);
    }

    #[tokio::test]
    pub async fn redirect_repo_mock_get_save() {
        let redirect_repo = RedirectRepoMock::new();
        let redirect = Redirect::new(
            0,
            10,
            Utc::now(),
            Utc::now(),
            String::from("redirect1"),
            String::from("https://example.com"),
        );
        let save_result = redirect_repo.save_or_update(&redirect).await;
        assert!(save_result.is_ok());
        assert_eq!(redirect_repo.get(0).await.unwrap(), redirect);
        assert_eq!(redirect_repo.get_by_drop_id(10).await.unwrap(), redirect);
    }
}
