use crate::repository::artist::Artist;
use crate::repository::drop::Drop;
use crate::repository::{Repo, RepoByName};
pub use crate::service::DropServiceT;
use async_trait::async_trait;
use derive_new::new;
use serde::Deserialize;
use std::fs;
use crate::repository::artwork::Artwork;

pub const ARTWORK_DIR_PREFIX: &str = "artwork_";
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
    ArtworkRepositoryIsNone,
    CantCreateArtistFromArtistName,
    CantCreateDropFromDropRequest,
    CantCreateArtworkFromArtworkName,
    CantCreateArtworkDirectoryInWebServer,
    CantCopyTrackFileToArtworkDirectory,
    NeitherArtistIdNorArtistNamePresent,
    CantFindArtistInRepository,
}

#[derive(Clone, Deserialize, new)]
pub struct DropRequest {
    artist_id: Option<i32>,
    artist_name: Option<String>,
    artwork_name: String,
    tracks: Vec<String>
}

impl DropRequest {
    pub fn artist_id(&self) -> &Option<i32> {
        &self.artist_id
    }

    pub fn artist_name(&self) -> &Option<String> {
        &self.artist_name
    }

    pub fn artwork_name(&self) -> &str {
        &self.artwork_name
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
    V: Repo<Artwork> + Send + Sync,
{
    drop_repository: T,
    artist_repository: U,
    artwork_repository: V,
}

impl<T, U, V> Clone for DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync + Clone,
    U: RepoByName<Artist> + Send + Sync + Clone,
    V: Repo<Artwork> + Send + Sync + Clone, {
    fn clone(&self) -> Self {
        DropService::new(
            self.drop_repository.clone(),
            self.artist_repository.clone(),
            self.artwork_repository.clone()
        )
    }
}

impl<T, U, V> DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync,
    U: RepoByName<Artist> + Send + Sync,
    V: Repo<Artwork> + Send + Sync,
{
    pub fn new(
        drop_repository: T,
        artist_repository: U,
        artwork_repository: V,
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
        if artwork_repository.is_none() {
            println!("artwork repository not set, can't create drop");
            return Err(ImportError::ArtworkRepositoryIsNone)
        }*/

        Self {
            drop_repository,
            artist_repository,
            artwork_repository,
        }
    }

    pub fn drop_repository(&self) -> &T {
        &self.drop_repository
    }

    pub fn artist_repository(&self) -> &U {
        &self.artist_repository
    }

    pub fn artwork_repository(&self) -> &V {
        &self.artwork_repository
    }
}

#[async_trait]
impl<T, U, V> DropServiceT for DropService<T, U, V>
where
    T: Repo<Drop> + Send + Sync,
    U: RepoByName<Artist> + Send + Sync,
    V: Repo<Artwork> + Send + Sync,
{
    async fn create_drop(
        &self,
        drop_import_path: &String,
        drop_request: DropRequest,
        web_server_path: &String
    ) -> Result<(), ImportError> {

        // artist_id XOR artist_name
        if drop_request.artist_id.is_some() && drop_request.artist_name.is_some() {
            return Err(ImportError::ArtistIdAndArtistNameAreBothPresent);
        }
        if drop_request.artist_id.is_none() && drop_request.artist_name.is_none() {
            return Err(ImportError::NeitherArtistIdNorArtistNamePresent);
        }
        let artist = if let Some(artist_id) = drop_request.artist_id {
            // check artist_id exists
            self.artist_repository.get(artist_id)
                .await
                .or(Err(ImportError::InvalidArtistId))?
        } else if let Some(artist_name) = drop_request.artist_name {
            // check artist_name exists
            self.artist_repository.get_by_name(&artist_name)
                .await
                .or(Err(ImportError::CantCreateArtistFromArtistName))?
        } else {
            return Err(ImportError::NeitherArtistIdNorArtistNamePresent);
        };

        // create artwork
        let now = chrono::Utc::now();
        let artwork_id = self.artwork_repository
            .save_or_update(&Artwork::new(0, artist.id(), now, now, drop_request.artwork_name))
            .await
            .or(Err(ImportError::CantCreateArtworkFromArtworkName))?;

        // create drop
        let _drop_id = self.drop_repository
            .save_or_update(&Drop::new(0, artwork_id, artist.name().to_string()))
            .await
            .or(Err(ImportError::CantCreateDropFromDropRequest))?;

        // create artwork directory in web server
        let mut artwork_dir_path = web_server_path.clone();
        artwork_dir_path.push_str("/");
        artwork_dir_path.push_str(ARTWORK_DIR_PREFIX);
        artwork_dir_path.push_str(&artwork_id.to_string());
        fs::create_dir(&artwork_dir_path).or(Err(ImportError::CantCreateArtworkDirectoryInWebServer))?;
        // move the files
        let mut i = 1;
        for track in drop_request.tracks.iter() {
            let mut track_import_path = drop_import_path.clone();
            track_import_path.push_str("/");
            track_import_path.push_str(track);
            let mut artwork_track_path = artwork_dir_path.clone();
            artwork_track_path.push_str("/");
            artwork_track_path.push_str(TRACK_FILE_PREFIX);
            artwork_track_path.push_str(&i.to_string());
            fs::copy(track_import_path, artwork_track_path)
                .or(Err(ImportError::CantCopyTrackFileToArtworkDirectory))?;
            i += 1;
        }
        Ok(())
    }
}