use crate::config::db::{DatabaseConfig, create_pool};
use crate::repository::{Entity, RepositoryError};
use chrono::NaiveDateTime;
use derive_new::new;
use sqlx::{Pool, Postgres};
use uuid::Uuid;

#[derive(sqlx::FromRow, Debug, Clone, PartialEq, new)]
pub struct Token {
    id: Uuid,
    create_date: NaiveDateTime,
    tag_id: i32
}

impl Token {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn create_date(&self) -> NaiveDateTime {
        self.create_date
    }

    pub fn tag_id(&self) -> i32 {
        self.tag_id
    }
}

impl Entity for Token {
    fn id(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Debug, Clone)]
pub struct TokenRepo {
    pub pool: Pool<Postgres>,
}

impl TokenRepo {
    pub async fn new(database_config: &DatabaseConfig) -> Result<TokenRepo, RepositoryError> {
        match create_pool(database_config).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => Err(RepositoryError::DatabaseError(err))
        }
    }

    pub fn from_pool(pool: Pool<Postgres>) -> Result<TokenRepo, RepositoryError> {
        Ok(Self { pool })
    }

    pub async fn get(&self, id: Uuid) -> Result<Token, RepositoryError> {
        sqlx::query_as::<_, Token>("
SELECT id, create_date, tag_id
FROM \"token\"
WHERE id = $1
LIMIT 1
")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(|err| {
                println!("Error fetching token: {:?}", err);
                RepositoryError::EntityNotFound
            })
    }
}
