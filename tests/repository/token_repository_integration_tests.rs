use crate::utils::{create_default_db_config, start_postgres_container};
use chrono::NaiveDateTime;
use drop_reverse_proxy::config::db::run_migrations;
use drop_reverse_proxy::repository::tag::{Tag, TagRepo};
use drop_reverse_proxy::repository::token::{Token, TokenRepo};
use drop_reverse_proxy::repository::RepoByName;
use uuid::Uuid;

#[path = "../utils.rs"]
mod utils;

#[tokio::test]
async fn test_token_repo_integration() {
    // 1. Start Postgres container
    let db_name = "drop_of_culture";
    let user = "drop_of_culture";
    let password = "drop_of_culture";
    let (_container_guard, host, port) = start_postgres_container(
        db_name,
        user,
        password,
    ).await.expect("Failed to start Postgres container");

    // 2. Setup database pool
    let db_config = create_default_db_config(host, port, db_name, user, password);

    let pool = drop_reverse_proxy::config::db::create_pool(&db_config)
        .await
        .expect("Failed to create database pool");

    // 3. Initialize schema from the migrations directory
    run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    // 4. A token references a tag, which references a drop/artwork/artist chain,
    //    so seed the whole chain first.
    let artist_id = sqlx::query_scalar::<_, i32>("
INSERT INTO \"artist\" (name)
VALUES ($1)
RETURNING id
")
        .bind("Test Artist")
        .fetch_one(&pool)
        .await
        .expect("Failed to insert artist");

    let artwork_id = sqlx::query_scalar::<_, i32>("
INSERT INTO \"artwork\" (artist_id, name)
VALUES ($1, $2)
RETURNING id
")
        .bind(artist_id)
        .bind("Test Artwork")
        .fetch_one(&pool)
        .await
        .expect("Failed to insert artwork");

    let drop_id = sqlx::query_scalar::<_, i32>("
INSERT INTO \"drop\" (artwork_id, name)
VALUES ($1, $2)
RETURNING id
")
        .bind(artwork_id)
        .bind("Test Drop")
        .fetch_one(&pool)
        .await
        .expect("Failed to insert drop");

    let tag_repo = TagRepo::from_pool(pool.clone())
        .expect("Failed to create tag repository");
    let new_tag = Tag::new(0, "Test Tag".to_string(), NaiveDateTime::default(), drop_id);
    let tag_id = tag_repo.save_or_update(&new_tag).await.expect("Failed to save tag");

    let repo = TokenRepo::from_pool(pool)
        .expect("Failed to create token repository");

    // 5. Test save_or_update
    let token_id = Uuid::new_v4();
    let new_token = Token::new(token_id, tag_id);

    let saved_token_id = repo.save_or_update(&new_token).await.expect("Failed to save token");
    assert_eq!(saved_token_id, token_id);

    // 6. Test get
    let saved_token = repo.get(saved_token_id).await.expect("Failed to get token");

    assert_eq!(saved_token.id(), token_id);
    assert_eq!(saved_token.tag_id(), tag_id);
}
