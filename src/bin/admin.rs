//! Back-office server, meant to run behind oauth2-proxy.
//!
//! Usage:
//!   admin                                  start the server
//!   admin add-admin <github_login> <role>  add (or re-activate) an admin, role = owner | editor
//!   admin disable-admin <github_login>     revoke an admin immediately

use drop_reverse_proxy::admin::{AdminState, admin_app, create_admin_conf_from_toml_file};
use drop_reverse_proxy::config::db::{DatabaseConfig, create_pool, run_migrations};
use drop_reverse_proxy::create_conf_from_toml_file;
use sqlx::PgPool;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let conf = create_conf_from_toml_file("app.toml")
        .expect("can't load conf from toml file");
    let admin_conf = create_admin_conf_from_toml_file("app.toml")
        .expect("admin_conf not found in app.toml");
    let db_conf = conf.db_conf().expect("db_conf not found in app.toml");
    let db_config = DatabaseConfig {
        host: db_conf.db_host().to_string(),
        port: db_conf.db_port(),
        database: db_conf.db_name().to_string(),
        username: db_conf.db_user().to_string(),
        password: db_conf.db_password().to_string(),
        schema: db_conf.db_schema().to_string(),
        max_connections: 5,
        min_connections: 1,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(100),
        max_lifetime: Duration::from_secs(1800)
    };

    let pool = create_pool(&db_config)
        .await
        .expect("can't connect to database");
    run_migrations(&pool)
        .await
        .expect("failed to run database migrations");

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => serve(pool, admin_conf.bind_addr(), admin_conf.allowed_origin(), conf.max_attempts()).await,
        ["add-admin", login, role @ ("owner" | "editor")] => add_admin(&pool, login, role).await,
        ["disable-admin", login] => disable_admin(&pool, login).await,
        _ => {
            eprintln!("usage: admin [add-admin <github_login> <owner|editor> | disable-admin <github_login>]");
            std::process::exit(2);
        }
    }
}

async fn serve(pool: PgPool, bind_addr: &str, allowed_origin: &str, max_attempts: u8) {
    let addr: SocketAddr = bind_addr.parse().expect("admin_conf.bind_addr is not a valid socket address");
    // the app trusts the X-Forwarded-User header: only oauth2-proxy must be able to reach it
    assert!(
        addr.ip().is_loopback(),
        "admin_conf.bind_addr must be a loopback address (got {addr}), the admin app trusts X-Forwarded-User"
    );

    let state = AdminState { pool, allowed_origin: Arc::from(allowed_origin), max_attempts };
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Admin listening on {addr}");
    axum::serve(listener, admin_app(state)).await.unwrap();
}

async fn add_admin(pool: &PgPool, login: &str, role: &str) {
    sqlx::query("
INSERT INTO \"admin_user\" (github_login, role, active)
VALUES ($1, $2, TRUE)
ON CONFLICT (github_login) DO UPDATE SET role = EXCLUDED.role, active = TRUE
")
        .bind(login)
        .bind(role)
        .execute(pool)
        .await
        .expect("failed to add admin");
    println!("{login} is now an active {role}");
}

async fn disable_admin(pool: &PgPool, login: &str) {
    let result = sqlx::query("UPDATE \"admin_user\" SET active = FALSE WHERE lower(github_login) = lower($1)")
        .bind(login)
        .execute(pool)
        .await
        .expect("failed to disable admin");
    if result.rows_affected() == 0 {
        eprintln!("{login} is not an admin");
        std::process::exit(1);
    }
    println!("{login} is disabled");
}
