use std::sync::Arc;
use async_trait::async_trait;
use chrono::NaiveDateTime;
use crate::config::db::{create_pool, DatabaseConfig};
use crate::repository::{Entity, RepoByName, RepositoryError};
use derive_new::new;
use sqlx::{Pool, Postgres};

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Tag {
    id: i32,
    name: String,
    create_date: NaiveDateTime,
    drop_id: i32,
}

impl Tag {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn create_date(&self) -> &NaiveDateTime {
        &self.create_date
    }
}

impl Entity for Tag {
    fn id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct TagRepo {
    pub pool: Pool<Postgres>,
}

impl TagRepo {
    pub async fn new(database_config: &DatabaseConfig) -> Result<TagRepo, RepositoryError> {
        match create_pool(database_config).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => Err(RepositoryError::DatabaseError(err))
        }
    }
}
#[async_trait]
impl RepoByName<Tag> for TagRepo {
    async fn get(&self, id: i32) -> Result<Tag, RepositoryError> {
        sqlx::query_as::<_, Tag>("
SELECT id, name, create_date
FROM \"tag\"
WHERE id = $1
LIMIT 1
")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotFound)
    }

    async fn save_or_update(&self, tag: &Tag) -> Result<i32, RepositoryError> {
        sqlx::query_scalar::<_, i32>("
INSERT INTO \"tag\" (name, create_date)
VALUES ($1, $2)
RETURNING id
    ")
            .bind(tag.name.clone())
            .bind(tag.create_date)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotSaved)
    }

    async fn get_by_name(&self, name: &str) -> Result<Tag, RepositoryError> {
        sqlx::query_as::<_, Tag>("
SELECT id, name, create_date
FROM \"tag\"
WHERE name = $1
LIMIT 1
")
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotFound)
    }
}

#[async_trait]
impl RepoByName<Tag> for Arc<TagRepo> {
    async fn get(&self, id: i32) -> Result<Tag, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &Tag) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }

    async fn get_by_name(&self, name: &str) -> Result<Tag, RepositoryError> {
        self.as_ref().get_by_name(name).await
    }
}
