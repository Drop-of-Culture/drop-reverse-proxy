use crate::utils::{create_default_db_config, start_postgres_container};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use drop_reverse_proxy::admin::{AdminState, admin_app};
use drop_reverse_proxy::config::db::{create_pool, run_migrations};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;

mod utils;

const ORIGIN: &str = "https://admin.example.com";

fn get(uri: &str, login: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder().uri(uri);
    if let Some(login) = login {
        builder = builder.header("x-forwarded-user", login);
    }
    builder.body(Body::empty()).unwrap()
}

fn post(uri: &str, login: &str, origin: &str, form: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("x-forwarded-user", login)
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(form.to_string()))
        .unwrap()
}

async fn status(app: &Router, req: Request<Body>) -> StatusCode {
    app.clone().oneshot(req).await.unwrap().status()
}

async fn artist_names(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT name FROM \"artist\" ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn test_admin_access_and_artist_crud() {
    let db_name = "drop_of_culture";
    let user = "drop_of_culture";
    let password = "drop_of_culture";
    let (_container_guard, host, port) = start_postgres_container(db_name, user, password)
        .await
        .expect("Failed to start Postgres container");
    let pool = create_pool(&create_default_db_config(host, port, db_name, user, password))
        .await
        .expect("Failed to create database pool");
    run_migrations(&pool).await.expect("Failed to run migrations");

    sqlx::query("
INSERT INTO \"admin_user\" (github_login, role, active)
VALUES ('the-owner', 'owner', TRUE), ('an-editor', 'editor', TRUE), ('gone', 'owner', FALSE)
")
        .execute(&pool)
        .await
        .unwrap();

    let app = admin_app(AdminState { pool: pool.clone(), allowed_origin: Arc::from(ORIGIN) });

    // authentication / authorization
    assert_eq!(status(&app, get("/artists", None)).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(&app, get("/artists", Some("stranger"))).await, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, get("/artists", Some("gone"))).await, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, get("/artists", Some("an-editor"))).await, StatusCode::OK);
    assert_eq!(status(&app, get("/artists", Some("The-Owner"))).await, StatusCode::OK);

    // cross-origin post is refused, nothing written
    let evil = post("/artists", "an-editor", "https://evil.example.com", "name=Evil");
    assert_eq!(status(&app, evil).await, StatusCode::FORBIDDEN);
    assert!(artist_names(&pool).await.is_empty());

    // create, with validation
    assert_eq!(status(&app, post("/artists", "an-editor", ORIGIN, "name=+++")).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/artists", "an-editor", ORIGIN, "name=+Bj%C3%B6rk+")).await, StatusCode::SEE_OTHER);
    assert_eq!(artist_names(&pool).await, vec!["Björk"]);
    let id: i32 = sqlx::query_scalar("SELECT id FROM \"artist\"").fetch_one(&pool).await.unwrap();

    // update
    let update = post(&format!("/artists/{id}"), "an-editor", ORIGIN, "name=Bjork");
    assert_eq!(status(&app, update).await, StatusCode::SEE_OTHER);
    assert_eq!(artist_names(&pool).await, vec!["Bjork"]);
    assert_eq!(status(&app, get(&format!("/artists/{id}/edit"), Some("an-editor"))).await, StatusCode::OK);
    assert_eq!(status(&app, get("/artists/999999/edit", Some("an-editor"))).await, StatusCode::NOT_FOUND);

    // delete: owner only, and not while artworks reference the artist
    let delete = |login| post(&format!("/artists/{id}/delete"), login, ORIGIN, "");
    assert_eq!(status(&app, delete("an-editor")).await, StatusCode::FORBIDDEN);
    sqlx::query("INSERT INTO \"artwork\" (artist_id, name) VALUES ($1, 'Homogenic')")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(status(&app, delete("the-owner")).await, StatusCode::CONFLICT);
    sqlx::query("DELETE FROM \"artwork\"").execute(&pool).await.unwrap();
    assert_eq!(status(&app, delete("the-owner")).await, StatusCode::SEE_OTHER);
    assert!(artist_names(&pool).await.is_empty());

    // every successful change is audited, failed ones are rolled back with their log
    let audit: Vec<(String, String, Option<serde_json::Value>, Option<serde_json::Value>)> = sqlx::query_as("
SELECT github_login, action, before, after
FROM \"audit_log\"
WHERE entity = 'artist' AND entity_id = $1
ORDER BY id
")
        .bind(id.to_string())
        .fetch_all(&pool)
        .await
        .unwrap();
    let actions: Vec<(&str, &str)> = audit.iter().map(|(login, action, _, _)| (login.as_str(), action.as_str())).collect();
    assert_eq!(actions, vec![("an-editor", "create"), ("an-editor", "update"), ("the-owner", "delete")]);
    assert_eq!(audit[1].2.as_ref().unwrap()["name"], "Björk");
    assert_eq!(audit[1].3.as_ref().unwrap()["name"], "Bjork");
    assert!(audit[2].3.is_none());
}
