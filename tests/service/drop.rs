use mock::repository::artist::ArtistRepoMock;
use mock::repository::drop::DropRepoMock;
use mock::repository::artwork::ArtworkRepoMock;
use mock::repository::playlist::PlaylistRepoMock;
use drop_reverse_proxy::repository::artist::Artist;
use drop_reverse_proxy::service::drop::{DropRequest, DropService, DropServiceT, ImportError, ARTWORK_DIR_PREFIX, TRACK_FILE_PREFIX};
use std::fs;
use sqlx::testing::TestTermination;
use tempfile::TempDir;
use drop_reverse_proxy::repository::Repo;

#[path = "../mock.rs"]
mod mock;

#[tokio::test]
async fn test_create_drop_success_with_artist_id() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let artist_id = 10;
    artist_repo.map_by_id().write().unwrap().insert(artist_id, Artist::new(artist_id, "Artist Name".to_string()));

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let temp_import_dir = TempDir::new().unwrap();
    let import_path = temp_import_dir.path().to_str().unwrap().to_string();
    fs::write(temp_import_dir.path().join("track1.mp3"), "content1").unwrap();

    let temp_web_server_dir = TempDir::new().unwrap();
    let web_server_path = temp_web_server_dir.path().to_str().unwrap().to_string();

    let drop_request = DropRequest::new(
        Some(artist_id),
        None,
        "Artwork Name".to_string(),
        vec!["track1.mp3".to_string()]
    );

    let result: Result<(), ImportError> = service.create_drop(&import_path, drop_request, &web_server_path).await;
    assert!(result.is_ok());

    // Verify the artwork directory and file
    // ArtworkRepoMock returns entity.id() on save. Artwork::new(0, ...) has id 0.
    let artwork_dir = temp_web_server_dir.path().join(format!("{}{}", ARTWORK_DIR_PREFIX, 0));
    assert!(artwork_dir.exists());
    assert!(artwork_dir.join(format!("{}{}", TRACK_FILE_PREFIX, 1)).exists());

    let drop_result = service.drop_repository().get(0).await;
    assert!(drop_result.is_ok());
    assert!(drop_result.unwrap().name().len() > 0);
}

#[tokio::test]
async fn test_create_drop_success_with_artist_name() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let artist_id = 10;
    let artist_name = "Artist Name";
    artist_repo.map_by_name().write().unwrap().insert(artist_name.to_string(), Artist::new(artist_id, artist_name.to_string()));

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let temp_import_dir = TempDir::new().unwrap();
    let import_path = temp_import_dir.path().to_str().unwrap().to_string();
    fs::write(temp_import_dir.path().join("track1.mp3"), "content1").unwrap();

    let temp_web_server_dir = TempDir::new().unwrap();
    let web_server_path = temp_web_server_dir.path().to_str().unwrap().to_string();

    let drop_request = DropRequest::new(
        None,
        Some(artist_name.to_string()),
        "Artwork Name".to_string(),
        vec!["track1.mp3".to_string()]
    );

    let result: Result<(), ImportError> = service.create_drop(&import_path, drop_request, &web_server_path).await;
    assert!(result.is_ok());

    let artwork_dir = temp_web_server_dir.path().join(format!("{}{}", ARTWORK_DIR_PREFIX, 0));
    assert!(artwork_dir.exists());
}

#[tokio::test]
async fn test_create_drop_error_both_artist_id_and_name() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let drop_request = DropRequest::new(
        Some(1),
        Some("Name".to_string()),
        "Playlist".to_string(),
        vec![]
    );

    let result = service.create_drop(&"import".to_string(), drop_request, &"web".to_string()).await;
    assert!(matches!(result, Err(ImportError::ArtistIdAndArtistNameAreBothPresent)));
}

#[tokio::test]
async fn test_create_drop_error_artist_id_not_found() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let drop_request = DropRequest::new(
        Some(999),
        None,
        "Artwork".to_string(),
        vec![]
    );

    let result = service.create_drop(&"import".to_string(), drop_request, &"web".to_string()).await;
    assert!(matches!(result, Err(ImportError::InvalidArtistId)));
}

#[tokio::test]
async fn test_create_drop_error_artist_name_not_found() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let drop_request = DropRequest::new(
        None,
        Some("Unknown".to_string()),
        "Playlist".to_string(),
        vec![]
    );

    let result = service.create_drop(&"import".to_string(), drop_request, &"web".to_string()).await;
    assert!(matches!(result, Err(ImportError::CantCreateArtistFromArtistName)));
}

#[tokio::test]
async fn test_create_drop_error_missing_track_file() {
    let artist_repo = ArtistRepoMock::new();
    let drop_repo = DropRepoMock::new();
    let artwork_repo = ArtworkRepoMock::new();

    let artist_id = 1;
    artist_repo.map_by_id().write().unwrap().insert(artist_id, Artist::new(artist_id, "Artist".to_string()));

    let service = DropService::new(drop_repo, artist_repo, artwork_repo);

    let temp_import_dir = TempDir::new().unwrap();
    let import_path = temp_import_dir.path().to_str().unwrap().to_string();
    // Do NOT create the track file

    let temp_web_server_dir = TempDir::new().unwrap();
    let web_server_path = temp_web_server_dir.path().to_str().unwrap().to_string();

    let drop_request = DropRequest::new(
        Some(artist_id),
        None,
        "Playlist".to_string(),
        vec!["missing.mp3".to_string()]
    );

    let result = service.create_drop(&import_path, drop_request, &web_server_path).await;
    assert!(matches!(result, Err(ImportError::CantCopyTrackFileToArtworkDirectory)));
}
