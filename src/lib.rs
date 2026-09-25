use crate::repository::artist::Artist;
use crate::repository::artwork::Artwork;
use crate::repository::token::Token;
use crate::repository::{Repo, RepoByName, RepositoryError};
use crate::service::DropServiceT;
use crate::service::drop::DropService;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use derive_new::new;
use figment::Figment;
use figment::providers::{Format, Toml};
use flate2::read::GzDecoder;
use regex::Regex;
use repository::RepoType;
use serde::{Deserialize, Serialize};
use service::drop::{DropRequest, ImportError};
use std::fs;
use std::fs::File;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tar::Archive;
use toml::de::Error;
use uuid::Uuid;

pub const TOKEN_NAME: &str = "dop_token";
pub const TAG_ARCHIVE_PREFIX: &str = "drop_";

pub mod repository;
pub mod service;
pub mod config;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route(
            "/tag/{tag}",
            get(tag).route_layer(axum::middleware::from_fn_with_state(state.clone(), tag_guard)),
        )
        .route(
            "/playlist",
            get(playlist).route_layer(axum::middleware::from_fn_with_state(state.clone(), token_guard))
        )
        .route(
            "/play",
            get(play).route_layer(axum::middleware::from_fn_with_state(state.clone(), token_guard))
        )
        .route(
            "/track/part/{file}",
            get(track_part).route_layer(axum::middleware::from_fn_with_state(state.clone(), token_guard))
        )
        .route(
            "/track/{track_number}",
            get(track).route_layer(axum::middleware::from_fn_with_state(state.clone(), token_guard))
        )
        .route(
            "/{*path}",
            get(file).route_layer(axum::middleware::from_fn_with_state(state.clone(), token_guard))
        )
        .route(
            "/drop/import",
            get(drop_import).route_layer(axum::middleware::from_fn_with_state(state.clone(), drop_import_guard)),
        )
        .route(
            "/",
            get(|| async { Ok::<_, StatusCode>(StatusCode::UNAUTHORIZED) })
        )
        // check route ""
        .with_state(state)
}

#[derive(Debug, Deserialize)]
enum AppError {
    TagNotFound,
    Unauthorized,
    InternalError,
    ResourceNotFound,
    PlaylistNotFound,
    TokenSaveError,
    DropNotFound,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // How we want errors responses to be serialized
        match &self {
            AppError::TagNotFound => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            AppError::Unauthorized => StatusCode::UNAUTHORIZED.into_response(),
            AppError::InternalError => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            AppError::ResourceNotFound => StatusCode::NOT_FOUND.into_response(),
            AppError::PlaylistNotFound => StatusCode::NOT_FOUND.into_response(),
            AppError::TokenSaveError => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            AppError::DropNotFound => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub token_repo: Arc<repository::token::TokenRepo>,
    pub tag_repo: Arc<repository::tag::TagRepo>,
    pub ip_repo: Arc<repository::ip::IpRepo>,
    pub conf: Conf,
    pub entity_repositories: Vec<RepoType>,
    pub service_conf: ServiceConf
}

async fn tag(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    Path(tag): Path<String>,
) -> Result<Response, AppError> {
    if let Some(tag_extracted) = extract_tag_from_path(tag.as_str()) {
        println!("tag(): tag extracted");
        let uuid = Uuid::new_v4();

        let tag = state.tag_repo.get_by_name(&tag_extracted).await.map_err(|_| AppError::TagNotFound)?;
        println!("tag(): tag found in repo");
        let drop = state.service_conf.drop_service.find_drop(tag.drop_id()).await.ok_or(AppError::DropNotFound)?;
        println!("tag(): drop found in repo");
        state.token_repo.save_or_update(&Token::new(uuid, tag.id())).await.map_err(|_| AppError::TokenSaveError)?;
        println!("token saved");

        let mut uri_new = state.conf.redirect_uri;
        uri_new.push_str("/tag/");
        uri_new.push_str(drop.dir());
        uri_new.push_str("/index.html");
        println!("calling url {uri_new}");
        return match reqwest::get(uri_new).await {
            Ok(resp) => {
                let mut response = resp.bytes().await.unwrap().into_response().into_body().into_response();
                let header_value_str = format!("{}={}", TOKEN_NAME, uuid);
                match HeaderValue::from_str(header_value_str.as_str()) {
                    Ok(header_value) => {
                        response.headers_mut().append(
                            SET_COOKIE,
                            header_value
                        );
                        Ok(response)
                    }
                    Err(_) => {
                        Err(AppError::InternalError)
                    }
                }
            },
            Err(_) => {
                increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
                Err(AppError::TagNotFound)
            },
        }
    }

    Err(AppError::TagNotFound)
}

async fn drop_import(
    State(state): State<AppState>
) -> Result<Response, AppError> {
    // check dir
    let import_path = state.conf.import_path;
    if import_path.is_empty() {
        println!("import_path not set, can't import");
        return Ok(StatusCode::FAILED_DEPENDENCY.into_response());
    }
    let path = std::path::Path::new(&import_path);
    if !path.is_dir() {
        println!("import_path is not a directory, can't import");
        return Ok(StatusCode::FAILED_DEPENDENCY.into_response());
    }
    // look for files
    let files_to_import = look_for_drop_files_at_path(&path);
    if files_to_import.is_empty() {
        println!("no files to import at import path");
        let _response = Response::builder()
            .status(StatusCode::OK)
            .body("{imported: 0}");
        return Ok(StatusCode::OK.into_response());
    }
    // check files
    for file in files_to_import {
        if let Ok((drop_import_path, drop_request)) = check_drop_file(&file) {
            state.service_conf.drop_service.create_drop(
                &drop_import_path,
                drop_request,
                &state.conf.web_server_path.as_ref().unwrap()
            )
                .await
                .or(Err(AppError::InternalError))?;
        }
    }

    Ok(StatusCode::OK.into_response())
}

pub fn check_drop_file(file: &str) -> Result<(String, DropRequest), ImportError> {
    if !file.ends_with(".tar.gz") {
        println!("file is not a tar.gz file");
        return Err(ImportError::InvalidFileExtension)
    }
    // create temporary dir
    let file_path = std::path::Path::new(file);
    let file_parent_option = file_path.parent();
    if file_parent_option.is_none() {
        return Err(ImportError::NoFileParentDirectory);
    }
    let file_parent_path = file_parent_option.unwrap();
    let in_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .or(Err(ImportError::InvalidUnixEpoch))?
        .as_millis();
    let untar_path_str = file_parent_path.to_str()
        .ok_or(ImportError::InvalidParentDirectory)?;
    let mut untar_path_string = String::from(untar_path_str);
    untar_path_string.push('_');
    untar_path_string.push_str(&*in_ms.to_string());
    fs::create_dir(&untar_path_string).or(Err(ImportError::CantCreateDropUntarDirectory))?;

    /*// copy to untar directory
    let mut copy_path_string = untar_path_string.clone();
    copy_path_string.push_str("/");
    copy_path_string.push_str(file_path.file_name().unwrap().to_str().unwrap());
    fs::copy(file, &copy_path_string).or(Err(ImportError::CantCopyToUntarDirectory))?;
*/
    // untar
    let tar_gz = File::open(file_path)
        .or(Err(ImportError::CantOpenDropFile))?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(&untar_path_string.as_str()).or(Err(ImportError::CantUnpackDropFile))?;

    // check files in untar dir
    check_unarchived_drop_files(&untar_path_string)
}

pub fn check_unarchived_drop_files(untar_path_string: &str) -> Result<(String, DropRequest), ImportError> {
    // check if drop.txt is present
    let drop_txt_path = untar_path_string.to_owned() + "/drop.txt";
    let mut drop_result = create_drop_request_from_toml_file(&drop_txt_path);
    let mut untar_path_string = String::from(untar_path_string);
    if drop_result.is_err() {
        fs::read_dir(&untar_path_string)
            .or(Err(ImportError::CantReadUntarDirectory))?
            .for_each(|dir_entry| {
            if let Ok(dir_entry) = dir_entry {
                if let Ok(file_type) = dir_entry.file_type() {
                    if file_type.is_dir() {
                        if let Some(file_name) = dir_entry.file_name().to_str() {
                            if !file_name.starts_with(".") {
                                if let Some(dir_entry_path) = dir_entry.path().to_str() {
                                    untar_path_string = String::from(dir_entry_path);
                                }
                            }
                        }
                    }
                }
            }
        });
        let drop_txt_path = untar_path_string.to_owned() + "/drop.txt";
        drop_result = create_drop_request_from_toml_file(&drop_txt_path);
        if drop_result.is_err() {
            return Err(ImportError::NoDropDescriptionFileFound);
        }
    }

    let drop = drop_result.unwrap();
    // check if tracks are present and valid files
    for track in drop.tracks() {
        if File::open(untar_path_string.to_owned() + "/" + &*track).is_err() {
            return Err(ImportError::MissingTrackInDropArchive)
        }
    }
    Ok((untar_path_string, drop))
}

pub fn look_for_drop_files_at_path(path: &std::path::Path) -> Vec<String> {
    match fs::read_dir(path) {
        Ok(read_dir) => {
            let mut files = Vec::new();
            for dir_entry in read_dir {
                if let Ok(entry) = dir_entry {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        if let Some(file_name) = entry_path.file_name() {
                            if let Some(file_name_str) = file_name.to_str() {
                                if file_name_str.starts_with(TAG_ARCHIVE_PREFIX) &&
                                    let Some(file_path) = entry_path.to_str() {
                                    files.push(file_path.to_string());
                                }
                            }
                        }
                    }
                }
            }
            files
        }
        Err(_) => {
            Vec::new()
        }
    }
}

// Route guard for /tag that validates the requested tag is allowed
async fn tag_guard(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next
) -> Response {
    println!("connect info ip {:#?}", connect_info.ip());
    // check if IP is banned
    if !check_ip(connect_info.ip(), &state.ip_repo, state.conf.max_attempts).await {
        increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
        return AppError::Unauthorized.into_response();
    }
    // check if tag exists
    let path = req.uri().path();
    if let Some(tag) = extract_tag_from_path(path) {
        if check_tag(tag.as_str(), state.tag_repo).await.is_ok() {
                let _ = state.ip_repo.save_or_update(&connect_info.ip(), 0).await;
                println!("tag_guard(): tag found in repo");
                return next.run(req).await.into_response();
        } else {
            println!("tag_guard(): tag not found in repo");
            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await
        }
    }
    println!("tag_guard(): tag not found in path");
    AppError::TagNotFound.into_response()
}

async fn drop_import_guard(
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next
) -> Response {
    if !connect_info.ip().to_string().starts_with("127") {
        return AppError::ResourceNotFound.into_response();
    }
    next.run(req).await
}

async fn increment_ip_nb_bad_attempts(ip_addr: &IpAddr, ip_repo: &Arc<repository::ip::IpRepo>) {
    if let Ok(ip) = ip_repo.get(ip_addr).await {
        let _ = ip_repo.save_or_update(ip_addr, *ip.nb_bad_attempts() + 1).await;
    }
}

// Placeholder for future token checks
async fn token_guard(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next
) -> Response {
    if !check_ip(connect_info.ip(), &state.ip_repo, state.conf.max_attempts).await {
        increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
        return AppError::Unauthorized.into_response();
    }
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME) {
        if let Ok(header_token_str) = header_token.to_str() {
            if let Ok(token_uuid_requested) = Uuid::parse_str(header_token_str) {
                if let Ok(_token) = state.token_repo.get(token_uuid_requested).await {
                    return next.run(req).await;
                }
            }
        }
    }
    increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
    AppError::Unauthorized.into_response()
}

async fn check_tag(tag: &str, tag_repo: Arc<repository::tag::TagRepo>) -> Result<crate::repository::tag::Tag, RepositoryError> {
    tag_repo.get_by_name(tag).await
}

async fn check_ip(ip_addr: IpAddr, ip_repo: &Arc<repository::ip::IpRepo>, max_bad_attempts: u8) -> bool {
    match ip_repo.get(&ip_addr).await {
        Ok(ip) => {
            println!("{:#?}", ip);
            *ip.nb_bad_attempts() < max_bad_attempts as u32
        },
        Err(_) => true,
    }
}

fn extract_tag_from_path(uri_path: &str) -> Option<String> {
    println!("match in {uri_path} ? ");
    let re = Regex::new(r"([^/]+)/?$").unwrap();
    if let Some(caps) = re.captures(uri_path) {
        let str = caps.get(1).unwrap().as_str().to_string();
        println!("match");
        Some(str)
    } else {
        println!("no match!");
        None
    }
}

async fn play(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
) -> Result<Response, AppError> {
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME) {
        if let Ok(token_str) = header_token.to_str() {
            if let Ok(token_uuid_requested) = Uuid::parse_str(token_str) {
                let token_opt = state.token_repo.get(token_uuid_requested).await;
                if let Ok(token) = token_opt
                    && let Ok(tag) = state.tag_repo.get(token.tag_id()).await
                    && let Some(drop) = state.service_conf.drop_service.find_drop(tag.drop_id()).await {
                    let mut uri_new = String::from(state.conf.redirect_uri);
                    uri_new.push_str("/tag/");
                    uri_new.push_str(drop.dir());
                    uri_new.push_str("/playlist.m3u8");
                    println!("calling {uri_new}");
                    return match reqwest::get(uri_new).await {
                        Ok(resp) => {
                            Ok(resp.bytes().await.unwrap().into_response())
                        },
                        Err(_) => {
                            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
                            Err(AppError::TagNotFound)
                        },
                    }
                }
            }
        }
    }
    Ok(StatusCode::UNAUTHORIZED.into_response())
}

async fn track(
    Path(track_number): Path<u8>,
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
) -> Result<Response, AppError> {
    println!("called : {}", req.uri().path());
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME)
        && let Ok(token_str) = header_token.to_str()
        && let Ok(token_uuid_requested) = Uuid::parse_str(token_str)
        && let Ok(token) = state.token_repo.get(token_uuid_requested).await
        && let Ok(tag) = state.tag_repo.get(token.tag_id()).await
        && let Some(drop) = state.service_conf.drop_service.find_drop(tag.drop_id()).await {

        let uri_new = format!("{}/tag/{}/{}_{}.m3u8", &state.conf.redirect_uri, drop.dir(), tag.name(), track_number);
        println!("calling {uri_new}");
        return match reqwest::get(uri_new).await {
            Ok(resp) => {
                Ok(resp.bytes().await.unwrap().into_response())
            },
            Err(_) => {
                increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
                Err(AppError::TagNotFound)
            },
        }
    }
    Ok(StatusCode::UNAUTHORIZED.into_response())
}

async fn track_part(
    Path(track_part): Path<String>,
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
) -> Result<Response, AppError> {
    file(State(state), ConnectInfo(connect_info), Path(track_part), req).await
}

async fn file(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    Path(path): Path<String>,
    req: Request,
) -> Result<Response, AppError> {
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME) {
        if let Ok(token_str) = header_token.to_str() {
            if let Ok(token_uuid_requested) = Uuid::parse_str(token_str) {
                let token_opt = state.token_repo.get(token_uuid_requested).await;
                if let Ok(token) = token_opt
                    && let Ok(tag) = state.tag_repo.get(token.tag_id()).await
                    && let Some(drop) = state.service_conf.drop_service.find_drop(tag.drop_id()).await {
                    let mut uri_new = String::from(state.conf.redirect_uri);
                    uri_new.push_str("/tag/");
                    uri_new.push_str(drop.dir());
                    uri_new.push('/');
                    uri_new.push_str(path.as_str());

                    println!("calling {uri_new}");
                    return match reqwest::get(uri_new).await {
                        Ok(resp) => {
                            resp.headers().iter().for_each(|(header_name, header_value)| {
                                println!("header: {:#?} - {:#?}", header_name, header_value);
                            });
                            Ok(resp.bytes().await.unwrap().into_response().into_body().into_response())
                        },
                        Err(_) => {
                            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
                            Err(AppError::TagNotFound)
                        },
                    }
                }
            }
        }
    }
    Ok(StatusCode::UNAUTHORIZED.into_response())
}

async fn playlist(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME)
        && let Ok(token_str) = header_token.to_str()
        && let Ok(token_uuid_requested) = Uuid::parse_str(token_str)
        && let Ok(token) = state.token_repo.get(token_uuid_requested).await
        && let Ok(tag) = state.tag_repo.get(token.tag_id()).await
        && let Some(drop) = state.service_conf.drop_service.find_drop(tag.drop_id()).await {

        let mut uri_new = String::from(&state.conf.redirect_uri);
        uri_new.push_str("/tag/");
        uri_new.push_str(drop.dir());
        uri_new.push_str("/playlist.toml");
        println!("checking if there is playlist info at uri: {uri_new}");
        return if let Ok(resp) = reqwest::get(uri_new).await
            && let Ok(text) = resp.text().await
            && !text.is_empty()
            && let Ok(playlist_data) = PlaylistData::create_from_toml_text(text.as_str()) {
            Json(playlist_data).into_response()
        } else {
            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo).await;
            AppError::PlaylistNotFound.into_response()
        }
    }
    AppError::Unauthorized.into_response()
}

#[derive(Clone, Deserialize, new, Debug)]
pub struct Conf {
    redirect_uri: String,
    bind_addr: String,
    max_attempts: u8,
    tags: Vec<String>,
    import_path: String,
    db_conf: Option<DbConf>,
    web_server_path: Option<String>,
}

impl Conf {

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn bind_addr(&self) -> &str {
        &self.bind_addr
    }

    pub fn max_attempts(&self) -> u8 {
        self.max_attempts
    }

    pub fn tags(&self) -> &Vec<String> {
        &self.tags
    }

    pub fn import_path(&self) -> &str { &self.import_path }

    pub fn db_conf(&self) -> Option<&DbConf> {
        self.db_conf.as_ref()
    }
}

pub fn create_conf_from_toml_file(relative_path: &str) -> figment::Result<Conf> {
    Figment::new()
        .merge(Toml::file(relative_path))
        .extract()
}

pub fn create_drop_request_from_toml_file(path: &str) -> figment::Result<DropRequest> {
    Figment::new()
        .merge(Toml::file(path))
        .extract()
}

#[derive(new)]
pub struct ServiceConf {
    drop_service: DropService<
        Arc<dyn Repo<repository::drop::Drop>>,
        Arc<dyn RepoByName<Artist>>,
        Arc<dyn Repo<Artwork>>,
    >,
}

impl Clone for ServiceConf {
    fn clone(&self) -> Self {
        Self {
            drop_service: self.drop_service.clone()
        }
    }
}

impl ServiceConf {
    pub fn drop_service(
        &self,
    ) -> &DropService<
        Arc<dyn Repo<repository::drop::Drop>>,
        Arc<dyn RepoByName<Artist>>,
        Arc<dyn Repo<Artwork>>,
    > {
        &self.drop_service
    }
}

#[derive(Clone, new, Deserialize, Debug)]
pub struct DbConf {
    db_host: String,
    db_port: u16,
    db_name: String,
    db_user: String,
    db_password: String,
    db_pool_size: u32,
    db_timeout: u64
}

impl DbConf {
    pub fn db_host(&self) -> &str {
        &self.db_host
    }

    pub fn db_port(&self) -> u16 {
        self.db_port
    }

    pub fn db_name(&self) -> &str {
        &self.db_name
    }
    
    pub fn db_user(&self) -> &str { &self.db_user }

    pub fn db_password(&self) -> &str {
        &self.db_password
    }

    pub fn db_pool_size(&self) -> u32 {
        self.db_pool_size
    }

    pub fn db_timeout(&self) -> u64 {
        self.db_timeout
    }
}

#[derive(Clone, Deserialize, new, Debug, Serialize)]
pub struct PlaylistData {
    artist_name: String,
    playlist_name: String,
    tracks: Vec<String>
}

impl PlaylistData {
    pub fn artist_name(&self) -> &str {
        &self.artist_name
    }
    pub fn playlist_name(&self) -> &str {
        &self.playlist_name
    }
    pub fn tracks(&self) -> &Vec<String> {
        &self.tracks
    }
    pub fn create_from_toml_text(toml_text: &str) -> Result<PlaylistData, Error> {
        toml::from_str(toml_text)
    }
}