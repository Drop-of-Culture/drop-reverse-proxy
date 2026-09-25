use crate::config::db::{DatabaseConfig, create_pool};
use crate::repository::{Entity, RepoByDropId, RepositoryError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use derive_new::new;
use sqlx::{Execute, Pool, Postgres};
use std::sync::Arc;

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Redirect {
    pub id: i32,
    pub drop_id: i32,
    pub create_date: DateTime<Utc>,
    pub update_date: DateTime<Utc>,
    pub name: String,
    pub link: String,
}

impl Redirect {
    pub fn id(&self) -> i32 {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn link(&self) -> &str {
        &self.link
    }
}

impl Entity for Redirect {
    fn id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct RedirectRepo {
    pub pool: Pool<Postgres>,
}

impl RedirectRepo {
    pub fn from_pool(pool: Pool<Postgres>) -> Result<RedirectRepo, RepositoryError> {
        Ok(Self { pool })
    }
    
    pub async fn new(database_config: &DatabaseConfig) -> Result<RedirectRepo, RepositoryError> {
        match create_pool(database_config).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => Err(RepositoryError::DatabaseError(err))
        }
    }
}

#[async_trait]
impl RepoByDropId<Redirect> for RedirectRepo {
    async fn get(&self, id: i32) -> Result<Redirect, RepositoryError> {
        sqlx::query_as::<_, Redirect>("
SELECT id, drop_id, create_date, update_date, name, link
FROM \"redirect\"
WHERE id = $1
LIMIT 1
")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotFound)
    }

    async fn save_or_update(&self, redirect: &Redirect) -> Result<i32, RepositoryError> {
        sqlx::query_scalar::<_, i32>("
INSERT INTO \"redirect\" (drop_id, name, link)
VALUES ($1, $2, $3)
RETURNING id
    ")
            .bind(redirect.drop_id)
            .bind(redirect.name.clone())
            .bind(redirect.link.clone())
            .fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotSaved)
    }

    async fn get_by_drop_id(&self, drop_id: i32) -> Result<Redirect, RepositoryError> {
        let req = sqlx::query_as::<_, Redirect>("
SELECT id, drop_id, create_date, update_date, name, link
FROM \"redirect\"
WHERE drop_id = $1
LIMIT 1
")
            .bind(drop_id);
        println!("get_by_drop_id(): {} - {}", req.sql(), drop_id);

            req.fetch_one(&self.pool)
            .await
            .map_err(|_| RepositoryError::EntityNotFound)
    }
}

#[async_trait]
impl RepoByDropId<Redirect> for Arc<RedirectRepo> {
    async fn get(&self, id: i32) -> Result<Redirect, RepositoryError> {
        self.as_ref().get(id).await
    }

    async fn save_or_update(&self, entity: &Redirect) -> Result<i32, RepositoryError> {
        self.as_ref().save_or_update(entity).await
    }

    async fn get_by_drop_id(&self, drop_id: i32) -> Result<Redirect, RepositoryError> {
        self.as_ref().get_by_drop_id(drop_id).await
    }
}