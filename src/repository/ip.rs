use crate::config::db::{DatabaseConfig, create_pool};
use crate::repository::{Entity, RepositoryError};
use chrono::NaiveDateTime;
use sqlx::{Pool, Postgres};
use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub struct Ip {
    addr: IpAddr,
    first_seen: NaiveDateTime,
    last_seen: NaiveDateTime,
    nb_bad_attempts: u32,
}

impl Ip {
    pub fn addr(&self) -> &IpAddr {
        &self.addr
    }

    pub fn first_seen(&self) -> &NaiveDateTime {
        &self.first_seen
    }

    pub fn last_seen(&self) -> &NaiveDateTime {
        &self.last_seen
    }

    pub fn nb_bad_attempts(&self) -> &u32 {
        &self.nb_bad_attempts
    }

    pub fn new(addr: IpAddr, first_seen: NaiveDateTime, last_seen: NaiveDateTime, nb_bad_attempts: u32) -> Self {
        Self { addr, first_seen, last_seen, nb_bad_attempts }
    }
}

impl Entity for Ip {
    fn id(&self) -> String {
        self.addr.to_string()
    }
}

#[derive(sqlx::FromRow, Debug, Clone, PartialEq)]
struct IpRow {
    addr: String,
    first_seen: NaiveDateTime,
    last_seen: NaiveDateTime,
    nb_bad_attempts: i32,
}

impl TryFrom<IpRow> for Ip {
    type Error = RepositoryError;

    fn try_from(row: IpRow) -> Result<Self, Self::Error> {
        Ok(Self {
            addr: IpAddr::from_str(&row.addr).map_err(|_| RepositoryError::EntityNotFound)?,
            first_seen: row.first_seen,
            last_seen: row.last_seen,
            nb_bad_attempts: row.nb_bad_attempts as u32,
        })
    }
}

#[derive(Debug, Clone)]
pub struct IpRepo {
    pub pool: Pool<Postgres>,
}

impl IpRepo {
    pub async fn new(database_config: &DatabaseConfig) -> Result<IpRepo, RepositoryError> {
        match create_pool(database_config).await {
            Ok(pool) => Ok(Self { pool }),
            Err(err) => Err(RepositoryError::DatabaseError(err))
        }
    }

    pub fn from_pool(pool: Pool<Postgres>) -> Result<IpRepo, RepositoryError> {
        Ok(Self { pool })
    }

    pub async fn get(&self, ip_addr: &IpAddr) -> Result<Ip, RepositoryError> {
        sqlx::query_as::<_, IpRow>("
SELECT addr, first_seen, last_seen, nb_bad_attempts
FROM \"ip\"
WHERE addr = $1
LIMIT 1
")
            .bind(ip_addr.to_string())
            .fetch_one(&self.pool)
            .await
            .map_err(|err| {
                println!("Error fetching ip: {:?}", err);
                RepositoryError::EntityNotFound
            })?
            .try_into()
    }

    pub async fn save_or_update(&self, ip_addr: &IpAddr, nb_bad_attempts: u32) -> Result<(), RepositoryError> {
        sqlx::query("
INSERT INTO \"ip\" (addr, first_seen, last_seen, nb_bad_attempts)
VALUES ($1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, $2)
ON CONFLICT (addr) DO UPDATE
SET last_seen = CURRENT_TIMESTAMP, nb_bad_attempts = EXCLUDED.nb_bad_attempts
")
            .bind(ip_addr.to_string())
            .bind(nb_bad_attempts as i32)
            .execute(&self.pool)
            .await
            .map(|_| ())
            .map_err(|_| RepositoryError::EntityNotSaved)
    }
}
