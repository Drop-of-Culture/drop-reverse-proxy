use std::sync::Arc;
use async_trait::async_trait;
use crate::config::db::{create_pool, DatabaseConfig};
use crate::repository::{Entity, Repo, RepositoryError};
use derive_new::new;
use sqlx::{Pool, Postgres};

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Drop {
    id: i32,
    artwork_id: i32,
    name: String,
}

impl Drop {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn artwork_id(&self) -> i32 {
        self.artwork_id
    }

    pub fn name(&self) -> &str {
        &self.name
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
SELECT id, artwork_id, name
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
INSERT INTO \"drop\" (artwork_id, name)
VALUES ($1, $2)
RETURNING id
    ")
            .bind(drop.artwork_id)
            .bind(drop.name.clone())
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
