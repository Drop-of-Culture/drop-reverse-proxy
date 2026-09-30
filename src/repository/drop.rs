use crate::config::db::{DatabaseConfig, create_pool};
use crate::repository::{Entity, Repo, RepoByToken, RepositoryError};
use async_trait::async_trait;
use derive_new::new;
use sqlx::{Execute, Pool, Postgres};
use std::sync::Arc;
use uuid::Uuid;

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Drop {
    id: i32,
    artwork_id: i32,
    name: String,
    dir: String,
    type_id: i32,
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

    pub fn dir(&self) -> &str {
        &self.dir
    }
    
    pub fn type_id(&self) -> i32 {
        self.type_id
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

    pub fn from_pool(pool: Pool<Postgres>) -> Result<DropRepo, RepositoryError> {
        Ok(Self { pool })
    }
}

#[async_trait]
impl Repo<Drop> for DropRepo {
    async fn get(&self, id: i32) -> Result<Drop, RepositoryError> {
        let req = sqlx::query_as::<_, Drop>("
SELECT id, artwork_id, name, dir, type_id
FROM \"drop\"
WHERE id = $1
LIMIT 1
")
            .bind(id);
            tracing::debug!(sql = req.sql(), id, "get drop");
            req.fetch_one(&self.pool)
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
INSERT INTO \"drop\" (artwork_id, name, dir, type_id)
VALUES ($1, $2, $3, $4)
RETURNING id
    ")
            .bind(drop.artwork_id)
            .bind(drop.name.clone())
            .bind(drop.dir.clone())
            .bind(drop.type_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotSaved)
    }
}

#[async_trait]
impl RepoByToken<Drop> for DropRepo {
    async fn get_by_token(&self, token_id: &Uuid) -> Result<Drop, RepositoryError> {
        sqlx::query_as::<_, Drop>("
SELECT d.id, d.artwork_id, d.name, d.dir, d.type_id
FROM \"token\" t
JOIN \"tag\" tg ON tg.id = t.tag_id
JOIN \"drop\" d ON d.id = tg.drop_id
WHERE t.id = $1
LIMIT 1
")
            .bind(token_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                match e {
                    sqlx::Error::RowNotFound => RepositoryError::EntityNotFound,
                    _ => RepositoryError::DatabaseError(e),
                }
            })
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

#[async_trait]
impl RepoByToken<Drop> for Arc<DropRepo> {
    async fn get_by_token(&self, token_id: &Uuid) -> Result<Drop, RepositoryError> {
        self.as_ref().get_by_token(token_id).await
    }
}
