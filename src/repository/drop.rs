use std::sync::Arc;
use async_trait::async_trait;
use crate::config::db::{create_pool, DatabaseConfig};
use crate::repository::{Entity, Repo, RepositoryError};
use derive_new::new;
use sqlx::{Pool, Postgres};

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Drop {
    id: i32,
    // TODO remove artist_id : the art_work has an artist, not the drop
    artist_id: i32,
    // TODO remove type_id : the art_work has a type, not the drop
    type_id: i16,
    artwork_id: i32,
}

impl Drop {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn artist_id(&self) -> i32 {
        self.artist_id
    }

    pub fn type_id(&self) -> i16 {
        self.type_id
    }

    pub fn artwork_id(&self) -> i32 {
        self.artwork_id
    }
}

impl Entity for Drop {
    fn id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct DropRepo {
    pool: Pool<Postgres>,
}

#[derive(Debug)]
pub enum DropRepoError {
    DatabaseError(sqlx::Error),
}

impl DropRepo {
    pub async fn new(database_config: &DatabaseConfig) -> Result<DropRepo, RepositoryError>  {
        match create_pool(database_config).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => Err(RepositoryError::DatabaseError(err))
        }

    }
    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }
}

#[async_trait]
impl Repo<Drop> for DropRepo {
    async fn get(&self, id: i32) -> Result<Drop, RepositoryError> {
        sqlx::query_as::<_, Drop>("
SELECT id, artist_id, type_id, artwork_id
FROM \"drop\"
WHERE id = $1
LIMIT 1
")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                match e {
                    sqlx::Error::RowNotFound => RepositoryError::EntityNotFound,
                    _ => RepositoryError::DatabaseError(e),
                }
            })
    }

    async fn save_or_update(&self, drop: &Drop) -> Result<i32, RepositoryError> {
        sqlx::query_scalar::<_, i32>("
INSERT INTO \"drop\" (artist_id, artwork_id, type_id)
VALUES ($1, $2, $3)
RETURNING id
    ")
            .bind(drop.artist_id)
            .bind(drop.artwork_id)
            .bind(drop.type_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotSaved)
    }
}

#[async_trait]
impl Repo<Drop> for Arc<DropRepo> {
    async fn get(&self, id: i32) -> Result<Drop, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &Drop) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }
}
