use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::time::Duration;

/// Database configuration
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            database: "drop_of_culture".to_string(),
            username: "doc".to_string(),
            password: "doc".to_string(),
            max_connections: 10,
            min_connections: 1,
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(600),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

/// Create a configured database pool
pub async fn create_pool(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    // Build connection options
    let connect_options = PgConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .database(&config.database)
        .username(&config.username)
        .password(&config.password)
        // Enable statement caching for better performance
        .statement_cache_capacity(256);

    // Build pool with configuration
    let pool = PgPoolOptions::new()
        // Maximum number of connections in the pool
        .max_connections(config.max_connections)
        // Minimum connections to keep open (warm pool)
        .min_connections(config.min_connections)
        // Timeout for acquiring a connection from pool
        .acquire_timeout(config.connect_timeout)
        // How long a connection can be idle before being closed
        .idle_timeout(Some(config.idle_timeout))
        // Maximum lifetime of a connection (prevents stale connections)
        .max_lifetime(Some(config.max_lifetime))
        // Run this SQL on every new connection
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                // Set session parameters
                sqlx::query("SET timezone = 'UTC'")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(connect_options)
        .await?;

    tracing::info!(
        max_connections = config.max_connections,
        min_connections = config.min_connections,
        "Database pool created"
    );

    Ok(pool)
}

/// Run pending migrations from the `migrations/` directory, creating any
/// missing tables. Safe to call on every startup: already-applied
/// migrations are skipped.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    tracing::info!("Database migrations up to date");
    println!("Database migrations up to date");
    Ok(())
}