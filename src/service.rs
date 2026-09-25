use crate::repository::artist::Artist;
use crate::repository::{Repo, RepoByName};
use crate::service::drop::{DropRequest, ImportError};
use async_trait::async_trait;
use uuid::Uuid;

pub mod drop;

pub trait ArtistRepoTrait: Repo<Artist> + RepoByName<Artist> + Send + Sync {}
impl<T: Repo<Artist> + RepoByName<Artist> + Send + Sync> ArtistRepoTrait for T {}

#[async_trait]
pub trait DropServiceT {
    async fn create_drop(
        &self,
        drop_import_path: &String,
        drop_request: DropRequest,
        web_server_path: &String
    ) -> Result<(), ImportError>;

    async fn find_drop(&self, id: i32) -> Option<drop::Drop>;
}
