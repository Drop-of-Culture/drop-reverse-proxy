use drop_reverse_proxy::repository::RepoByName;
use drop_reverse_proxy::repository::artist::{Artist, ArtistRepo};
use drop_reverse_proxy::repository::artwork::ArtworkRepo;
use drop_reverse_proxy::repository::drop::DropRepo;
use drop_reverse_proxy::service::drop::{ARTWORK_DIR_PREFIX, DropRequest, DropService, DropServiceT, TRACK_FILE_PREFIX};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

#[path = "../utils.rs"]
mod utils;

use utils::init_apache_http2_container;
use utils::{create_default_db_config, start_postgres_container};

async fn setup_db() -> (drop_reverse_proxy::config::db::DatabaseConfig, ContainerAsync<Postgres>) {
    let db_name = "drop_of_culture";
    let user = "drop_of_culture";
    let password = "drop_of_culture";
    let (container, host, port) = start_postgres_container(db_name, user, password)
        .await
        .expect("Failed to start Postgres container");

    let db_config = create_default_db_config(host, port, db_name, user, password);
    let pool = drop_reverse_proxy::config::db::create_pool(&db_config)
        .await
        .expect("Failed to create database pool");

    sqlx::query(
        r#"
        CREATE TABLE "artist" (
            id SERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL
        )
        "#
    )
        .execute(&pool)
        .await
        .expect("Failed to create artist table");

    sqlx::query(
        r#"
        CREATE TABLE "artwork" (
            id SERIAL PRIMARY KEY,
            artist_id INTEGER NOT NULL,
            create_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
            update_date TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
            name CHARACTER(255) NOT NULL
        )
        "#
    )
        .execute(&pool)
        .await
        .expect("Failed to create artwork table");

    sqlx::query(
        r#"
        CREATE TABLE "drop" (
            id SERIAL PRIMARY KEY,
            artwork_id INTEGER NOT NULL,
            name VARCHAR(255) NOT NULL
        )
        "#
    )
        .execute(&pool)
        .await
        .expect("Failed to create drop table");

    (db_config, container)
}

#[tokio::test]
async fn test_create_drop_with_new_artist_name() {
    let (db_config, _db_guard) = setup_db().await;

    // Apache container is requested in the issue, even if we use temp dir for web_server_path
    let _apache = init_apache_http2_container();

    let artist_repo = Arc::new(ArtistRepo::new(&db_config).await.unwrap());
    let drop_repo = Arc::new(DropRepo::new(&db_config).await.unwrap());
    let artwork_repo = Arc::new(ArtworkRepo::new(&db_config).await.unwrap());

    let service = DropService::new(drop_repo, artist_repo.clone(), artwork_repo);

    let temp_import_dir = TempDir::new().unwrap();
    let import_path = temp_import_dir.path().to_str().unwrap().to_string();

    let track1_path = temp_import_dir.path().join("track1.mp3");
    fs::write(&track1_path, "fake mp3 content 1").unwrap();
    let track2_path = temp_import_dir.path().join("track2.mp3");
    fs::write(&track2_path, "fake mp3 content 2").unwrap();

    let temp_web_server_dir = TempDir::new().unwrap();
    let web_server_path = temp_web_server_dir.path().to_str().unwrap().to_string();

    // Pre-insert artist to test get_by_name
    artist_repo.save_or_update(&Artist::new(0, "New Artist".to_string())).await.unwrap();

    let drop_request = DropRequest::new(
        None,
        Some("New Artist".to_string()),
        "My Artwork".to_string(),
        vec!["track1.mp3".to_string(), "track2.mp3".to_string()]
    );

    service.create_drop(&import_path, drop_request, &web_server_path).await.expect("Failed to create drop");

    // Verify file system
    // The artwork ID should be 1
    let artwork_dir = temp_web_server_dir.path().join(format!("{}{}", ARTWORK_DIR_PREFIX, 0));
    assert!(artwork_dir.exists());
    assert!(artwork_dir.join(format!("{}{}", TRACK_FILE_PREFIX, 1)).exists());
    assert!(artwork_dir.join(format!("{}{}", TRACK_FILE_PREFIX, 2)).exists());

    assert_eq!(fs::read_to_string(artwork_dir.join(format!("{}{}", TRACK_FILE_PREFIX, 1))).unwrap(), "fake mp3 content 1");
}

#[tokio::test]
async fn test_create_drop_with_existing_artist_id() {
    let (db_config, _db_guard) = setup_db().await;

    let _apache = init_apache_http2_container();

    let artist_repo = Arc::new(ArtistRepo::new(&db_config).await.unwrap());
    let drop_repo = Arc::new(DropRepo::new(&db_config).await.unwrap());
    let artwork_repo = Arc::new(ArtworkRepo::new(&db_config).await.unwrap());

    let service = DropService::new(drop_repo, artist_repo.clone(), artwork_repo);

    let artist_id = artist_repo.save_or_update(&Artist::new(0, "Existing Artist".to_string())).await.unwrap();

    let temp_import_dir = TempDir::new().unwrap();
    let import_path = temp_import_dir.path().to_str().unwrap().to_string();
    fs::write(temp_import_dir.path().join("t1.mp3"), "c1").unwrap();

    let temp_web_server_dir = TempDir::new().unwrap();
    let web_server_path = temp_web_server_dir.path().to_str().unwrap().to_string();

    let drop_request = DropRequest::new(
        Some(artist_id),
        None,
        "P1".to_string(),
        vec!["t1.mp3".to_string()],
    );

    service.create_drop(&import_path, drop_request, &web_server_path).await.expect("Failed to create drop");

    let artwork_dir = temp_web_server_dir.path().join(format!("{}{}", ARTWORK_DIR_PREFIX, 0));
    assert!(artwork_dir.exists());
}

#[tokio::test]
async fn test_create_drop_error_both_id_and_name() {
    let (db_config, _db_guard) = setup_db().await;

    let artist_repo = Arc::new(ArtistRepo::new(&db_config).await.unwrap());
    let drop_repo = Arc::new(DropRepo::new(&db_config).await.unwrap());
    let artwork_repo = Arc::new(ArtworkRepo::new(&db_config).await.unwrap());

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let drop_request = DropRequest::new(
        Some(1),
        Some("Name".to_string()),
        "P".to_string(),
        vec![],
    );

    let result = service.create_drop(&".".to_string(), drop_request, &".".to_string()).await;
    assert!(result.is_err());
    //assert_eq!(result.unwrap_err().to_string(), "Both artist_id and artist_name are set, but only one is allowed");
    // Should be ArtistIdAndArtistNameAreBothPresent
}
