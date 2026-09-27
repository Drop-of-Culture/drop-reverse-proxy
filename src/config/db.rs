use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::time::Duration;

pub const DEFAULT_SCHEMA: &str = "public";

/// Database configuration
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    /// Schema holding the tables, also where migrations are applied.
    pub schema: String,
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
            schema: DEFAULT_SCHEMA.to_string(),
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
        // Pin the schema: by default Postgres resolves unqualified names with
        // `"$user", public`, so which schema is used would depend on which exist.
        .options([("search_path", config.schema.as_str())])
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

    ensure_schema_exists(&pool, &config.schema).await?;

    tracing::info!(
        max_connections = config.max_connections,
        min_connections = config.min_connections,
        "Database pool created"
    );

    Ok(pool)
}

/// Creates the schema when missing (fresh database), so migrations have somewhere to go.
async fn ensure_schema_exists(pool: &PgPool, schema: &str) -> Result<(), sqlx::Error> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)",
    )
        .bind(schema)
        .fetch_one(pool)
        .await?;
    if !exists {
        let quoted = format!("\"{}\"", schema.replace('"', "\"\""));
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {quoted}"))
            .execute(pool)
            .await?;
        tracing::info!(schema, "Database schema created");
    }
    Ok(())
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