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

    let app = admin_app(AdminState { pool: pool.clone(), allowed_origin: Arc::from(ORIGIN), max_attempts: 3 });

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

async fn body(app: &Router, req: Request<Body>) -> String {
    let response = app.clone().oneshot(req).await.unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn test_admin_other_entities() {
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
    sqlx::query("INSERT INTO \"admin_user\" (github_login, role) VALUES ('the-owner', 'owner'), ('an-editor', 'editor')")
        .execute(&pool)
        .await
        .unwrap();
    let app = admin_app(AdminState { pool: pool.clone(), allowed_origin: Arc::from(ORIGIN), max_attempts: 3 });
    let scalar = |query: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i32>(query).fetch_one(&pool).await.unwrap() }
    };

    // every list page renders, admins is for owners only
    for page in ["/", "/artists", "/artworks", "/drops", "/drop-types", "/tags", "/redirects", "/playlists", "/tokens", "/ips", "/audit"] {
        assert_eq!(status(&app, get(page, Some("an-editor"))).await, StatusCode::OK, "{page}");
    }
    assert_eq!(status(&app, get("/admins", Some("an-editor"))).await, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, get("/admins", Some("the-owner"))).await, StatusCode::OK);

    // artwork: its artist must exist
    assert_eq!(status(&app, post("/artists", "an-editor", ORIGIN, "name=Moebius")).await, StatusCode::SEE_OTHER);
    let artist_id = scalar("SELECT id FROM \"artist\"").await;
    assert_eq!(status(&app, post("/artworks", "an-editor", ORIGIN, "artist_id=999&name=Arzach")).await, StatusCode::UNPROCESSABLE_ENTITY);
    let artwork = format!("artist_id={artist_id}&name=Arzach");
    assert_eq!(status(&app, post("/artworks", "an-editor", ORIGIN, &artwork)).await, StatusCode::SEE_OTHER);
    let artwork_id = scalar("SELECT id FROM \"artwork\"").await;

    // drop, of the seeded redirect type
    let drop = format!("artwork_id={artwork_id}&type_id=2&name=Arzach+drop&dir=");
    assert_eq!(status(&app, post("/drops", "an-editor", ORIGIN, &drop)).await, StatusCode::SEE_OTHER);
    let drop_id = scalar("SELECT id FROM \"drop\"").await;
    assert!(body(&app, get("/drops", Some("an-editor"))).await.contains("Arzach drop"));

    // tags: url-safe, unique, generated when empty
    let tag = |name: &str| format!("drop_id={drop_id}&name={name}");
    assert_eq!(status(&app, post("/tags", "an-editor", ORIGIN, &tag("a%2Fb"))).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/tags", "an-editor", ORIGIN, &tag("arzach"))).await, StatusCode::SEE_OTHER);
    assert_eq!(status(&app, post("/tags", "an-editor", ORIGIN, &tag("arzach"))).await, StatusCode::CONFLICT);
    assert_eq!(status(&app, post("/tags", "an-editor", ORIGIN, &tag(""))).await, StatusCode::SEE_OTHER);
    let generated: String = sqlx::query_scalar("SELECT name FROM \"tag\" WHERE name <> 'arzach'").fetch_one(&pool).await.unwrap();
    assert_eq!(generated.len(), 16);

    // redirect: http(s) links only
    let redirect = |link: &str| format!("drop_id={drop_id}&name=site&link={link}");
    assert_eq!(status(&app, post("/redirects", "an-editor", ORIGIN, &redirect("javascript%3Aalert(1)"))).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/redirects", "an-editor", ORIGIN, &redirect("https%3A%2F%2Fexample.com"))).await, StatusCode::SEE_OTHER);

    // playlist
    let playlist = format!("drop_id={drop_id}&name=Side+A");
    assert_eq!(status(&app, post("/playlists", "an-editor", ORIGIN, &playlist)).await, StatusCode::SEE_OTHER);

    // drop types: owners only, can't delete one in use
    assert_eq!(status(&app, post("/drop-types", "an-editor", ORIGIN, "id=5&name=video")).await, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, post("/drop-types", "the-owner", ORIGIN, "id=-1&name=video")).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/drop-types", "the-owner", ORIGIN, "id=5&name=video")).await, StatusCode::SEE_OTHER);
    assert_eq!(status(&app, post("/drop-types/2/delete", "the-owner", ORIGIN, "")).await, StatusCode::CONFLICT);
    assert_eq!(status(&app, post("/drop-types/5/delete", "the-owner", ORIGIN, "")).await, StatusCode::SEE_OTHER);

    // a drop still used by tags can't be deleted
    assert_eq!(status(&app, post(&format!("/drops/{drop_id}/delete"), "the-owner", ORIGIN, "")).await, StatusCode::CONFLICT);

    // tokens can be revoked
    let token: String = sqlx::query_scalar("
INSERT INTO \"token\" (id, tag_id) SELECT $1, id FROM \"tag\" WHERE name = 'arzach' RETURNING id::text
")
        .bind(uuid::Uuid::new_v4())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(body(&app, get("/tokens", Some("an-editor"))).await.contains(&token));
    assert_eq!(status(&app, post(&format!("/tokens/{token}/delete"), "an-editor", ORIGIN, "")).await, StatusCode::SEE_OTHER);
    assert_eq!(scalar("SELECT COUNT(*)::int FROM \"token\"").await, 0);

    // banned ips can be unbanned
    sqlx::query("INSERT INTO \"ip\" (addr, nb_bad_attempts) VALUES ('203.0.113.7', 5)").execute(&pool).await.unwrap();
    assert!(body(&app, get("/ips", Some("an-editor"))).await.contains("banned"));
    assert_eq!(status(&app, post("/ips/203.0.113.7/unban", "an-editor", ORIGIN, "")).await, StatusCode::SEE_OTHER);
    assert_eq!(scalar("SELECT nb_bad_attempts FROM \"ip\"").await, 0);

    // admins: owners add admins, but can't lock themselves out
    assert_eq!(status(&app, post("/admins", "an-editor", ORIGIN, "github_login=someone&role=owner")).await, StatusCode::FORBIDDEN);
    assert_eq!(status(&app, post("/admins", "the-owner", ORIGIN, "github_login=-bad-&role=editor")).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/admins", "the-owner", ORIGIN, "github_login=new-editor&role=editor")).await, StatusCode::SEE_OTHER);
    assert_eq!(status(&app, get("/artists", Some("new-editor"))).await, StatusCode::OK);
    assert_eq!(status(&app, post("/admins/the-owner", "the-owner", ORIGIN, "role=editor&active=true")).await, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(status(&app, post("/admins/new-editor", "the-owner", ORIGIN, "role=editor&active=false")).await, StatusCode::SEE_OTHER);
    assert_eq!(status(&app, get("/artists", Some("new-editor"))).await, StatusCode::FORBIDDEN);

    // everything above is in the audit log, which can be filtered
    let entities: Vec<String> = sqlx::query_scalar("SELECT DISTINCT entity FROM \"audit_log\" ORDER BY entity").fetch_all(&pool).await.unwrap();
    assert_eq!(entities, vec!["admin_user", "artist", "artwork", "drop", "drop_type", "ip", "playlist", "redirect", "tag", "token"]);
    let tags_only = body(&app, get("/audit?entity=tag", Some("an-editor"))).await;
    assert!(tags_only.contains("arzach") && !tags_only.contains("Moebius"));
}
