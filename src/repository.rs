use async_trait::async_trait;
use std::sync::Arc;

pub mod drop;
pub mod artist;
pub mod playlist;
pub mod artwork;
pub mod tag;

pub trait Entity {
    fn id(&self) -> String;
}
#[derive(Debug)]
pub enum RepositoryError {
    EntityNotFound,
    EntityNotSaved,
    DatabaseError(sqlx::Error),
}
#[async_trait]
pub trait Repo<E: Entity>: Send + Sync {
    async fn get(&self, id: i32) -> Result<E, RepositoryError>;
    async fn save_or_update(&self, entity: &E) -> Result<i32, RepositoryError>;
}

#[async_trait]
impl<E: Entity + Sync> Repo<E> for Box<dyn Repo<E>> {
    async fn get(&self, id: i32) -> Result<E, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &E) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }
}

#[async_trait]
impl<E: Entity + Sync> Repo<E> for Arc<dyn Repo<E>> {
    async fn get(&self, id: i32) -> Result<E, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &E) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }
}

#[async_trait]
pub trait RepoByName<E: Entity>: Send + Sync {
    async fn get(&self, id: i32) -> Result<E, RepositoryError>;
    async fn save_or_update(&self, entity: &E) -> Result<i32, RepositoryError>;
    async fn get_by_name(&self, name: &str) -> Result<E, RepositoryError>;
}

#[async_trait]
impl<E: Entity + std::marker::Sync> RepoByName<E> for Arc<dyn RepoByName<E>> {
    async fn get(&self, id: i32) -> Result<E, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &E) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }
    async fn get_by_name(&self, name: &str) -> Result<E, RepositoryError> {
        self.as_ref().get_by_name(name).await
    }
}

#[derive(Clone, Debug)]
pub enum RepoType {
    Artist(std::sync::Arc<crate::repository::artist::ArtistRepo>),
    Drop(std::sync::Arc<crate::repository::drop::DropRepo>),
    Playlist(std::sync::Arc<crate::repository::playlist::PlaylistRepo>),
    Tag(std::sync::Arc<crate::repository::tag::TagRepo>),
}


