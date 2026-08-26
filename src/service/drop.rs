use crate::repository::artist::Artist;
use crate::repository::drop::Drop;
use crate::repository::playlist::Playlist;
use crate::repository::{Repo, RepoByName};
pub use crate::service::DropServiceT;
use async_trait::async_trait;
use serde::Deserialize;
use std::fs;
use derive_new::new;

pub const PLAYLIST_DIR_PREFIX: &str = "playlist_";
pub const TRACK_FILE_PREFIX: &str = "track_";

#[derive(Debug)]
pub enum ImportError {
    InvalidFileExtension,
    InvalidUnixEpoch,
    NoFileParentDirectory,
    InvalidParentDirectory,
    CantCreateDropUntarDirectory,
    CantCopyToUntarDirectory,
    CantOpenDropFile,
    CantUnpackDropFile,
    CantReadUntarDirectory,
    NoDropDescriptionFileFound,
    MissingTrackInDropArchive,
    ArtistIdAndArtistNameAreBothPresent,
    InvalidArtistId,
    DropRepositoryIsNone,
    ArtistRepositoryIsNone,
    PlaylistRepositoryIsNone,
    CantCreateArtistFromArtistName,
    CantCreateDropFromDropRequest,
    CantCreatePlaylistFromPlaylistName,
    CantCreatPlaylistDirectoryInWebServer,
    CantCopyTrackFileToPlaylistDirectory,
}

#[derive(Clone, Deserialize, new)]
pub struct DropRequest {
    artist_id: Option<i32>,
    artist_name: Option<String>,
    playlist_name: String,
    tracks: Vec<String>
}

impl DropRequest {
    pub fn artist_id(&self) -> &Option<i32> {
        &self.artist_id
    }

    pub fn artist_name(&self) -> &Option<String> {
        &self.artist_name
    }

    pub fn playlist_name(&self) -> &str {
        &self.playlist_name
    }

    pub fn tracks(&self) -> &Vec<String> {
        &self.tracks
    }
}

#[derive(Debug, Deserialize,)]
pub struct DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync,
    U: RepoByName<Artist> + Send + Sync,
    V: Repo<Playlist> + Send + Sync,
{
    drop_repository: T,
    artist_repository: U,
    playlist_repository: V,
}

impl<T, U, V> Clone for DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync + Clone,
    U: RepoByName<Artist> + Send + Sync + Clone,
    V: Repo<Playlist> + Send + Sync + Clone, {
    fn clone(&self) -> Self {
        DropService::new(
            self.drop_repository.clone(),
            self.artist_repository.clone(),
            self.playlist_repository.clone()
        )
    }
}

impl<T, U, V> DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync,
    U: RepoByName<Artist> + Send + Sync,
    V: Repo<Playlist> + Send + Sync,
{
    pub fn new(
        drop_repository: T,
        artist_repository: U,
        playlist_repository: V,
    ) -> DropService<T, U, V>
    where
        T: Sized,
        U: Sized,
        V: Sized,
    {
        /*if drop_repository.drop() {
            println!("drop repository not set, can't create drop");
            return Err(ImportError::DropRepositoryIsNone)
        }
        if artist_repository.is_none() {
            println!("artist repository not set, can't create drop");
            return Err(ImportError::ArtistRepositoryIsNone)
        }
        if playlist_repository.is_none() {
            println!("playlist repository not set, can't create drop");
            return Err(ImportError::PlaylistRepositoryIsNone)
        }*/

        Self {
            drop_repository,
            artist_repository,
            playlist_repository,
        }
    }

    pub fn drop_repository(&self) -> &T {
        &self.drop_repository
    }

    pub fn artist_repository(&self) -> &U {
        &self.artist_repository
    }

    pub fn playlist_repository(&self) -> &V {
        &self.playlist_repository
    }
}

#[async_trait]
impl<T, U, V> DropServiceT for DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync,
    U: RepoByName<Artist> + Send + Sync,
    V: Repo<Playlist> + Send + Sync,
{
    async fn create_drop(
        &self,
        drop_import_path: &String,
        drop_request: DropRequest,
        web_server_path: &String
    ) -> Result<(), ImportError> {

        // artist_id XOR artist_name
        if drop_request.artist_id.is_some() && drop_request.artist_name.is_some() {
            return Err(ImportError::ArtistIdAndArtistNameAreBothPresent)
        }
        let mut drop_artist_id = 0;
        // artist_id exists
        if let Some(artist_id) = drop_request.artist_id {
            // check artist_id exists
            drop_artist_id = self.artist_repository.get(artist_id)
                .await
                .or(Err(ImportError::InvalidArtistId))?.id();
        } else if let Some(artist_name) = drop_request.artist_name {
            // check artist_name exists
            drop_artist_id = self.artist_repository.get_by_name(&artist_name)
                .await
                .or(Err(ImportError::CantCreateArtistFromArtistName))?.id();
        }

        // create drop
        let drop_id = self.drop_repository
            .save_or_update(&Drop::new(0, drop_artist_id, 0, 0))
            .await
            .or(Err(ImportError::CantCreateDropFromDropRequest))?;

        // create playlist
        let now = chrono::Utc::now();
        let playlist_id = self.playlist_repository
            .save_or_update(&Playlist::new(0, drop_id, now, now, drop_request.playlist_name))
            .await
            .or(Err(ImportError::CantCreatePlaylistFromPlaylistName))?;

        // create playlist directory in web server
        let mut playlist_dir_path = web_server_path.clone();
        playlist_dir_path.push_str("/");
        playlist_dir_path.push_str(PLAYLIST_DIR_PREFIX);
        playlist_dir_path.push_str(&playlist_id.to_string());
        fs::create_dir(&playlist_dir_path).or(Err(ImportError::CantCreatePlaylistFromPlaylistName))?;
        // move the files
        let mut i = 1;
        for track in drop_request.tracks.iter() {
            let mut track_import_path = drop_import_path.clone();
            track_import_path.push_str("/");
            track_import_path.push_str(track);
            let mut playlist_track_path = playlist_dir_path.clone();
            playlist_track_path.push_str("/");
            playlist_track_path.push_str(TRACK_FILE_PREFIX);
            playlist_track_path.push_str(&i.to_string());
            fs::copy(track_import_path, playlist_track_path)
                .or(Err(ImportError::CantCopyTrackFileToPlaylistDirectory))?;
            i += 1;
        }
        Ok(())
    }
}