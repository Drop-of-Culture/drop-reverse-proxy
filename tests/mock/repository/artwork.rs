use async_trait::async_trait;
use drop_reverse_proxy::repository::artwork::Artwork;
use drop_reverse_proxy::repository::{Repo, RepositoryError};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ArtworkRepoMock {
    map: Arc<RwLock<HashMap<i32, Artwork>>>
}
impl ArtworkRepoMock {
    pub fn new() -> Self {
        Self { map: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn map(&self) -> &Arc<RwLock<HashMap<i32, Artwork>>> {
        &self.map
    }
}

#[async_trait]
impl Repo<Artwork> for ArtworkRepoMock {
    async fn get(&self, id: i32) -> Result<Artwork, RepositoryError> {
        match self.map().read().unwrap().get(&id) {
            Some(drop) => Ok(drop.clone()),
            None => Err(RepositoryError::EntityNotFound)
        }
    }

    async fn save_or_update(&self, entity: &Artwork) -> Result<i32, RepositoryError> {
        self.map.write().unwrap().insert(entity.id(), entity.clone());
        Ok(entity.id())
    }
}