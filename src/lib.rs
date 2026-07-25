use crate::repository::artist::Artist;
use crate::repository::playlist::Playlist;
use crate::repository::{Repo, RepoByName};
use crate::service::drop::DropService;
use crate::service::DropServiceT;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDateTime;
use derive_new::new;
use figment::providers::{Format, Toml};
use figment::Figment;
use flate2::read::GzDecoder;
use redis::Commands;
use regex::Regex;
use repository::RepoType;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use service::drop::{DropRequest, ImportError};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
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
    PlaylistNotFound
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
        }
    }
}

#[derive(Debug, Clone, new)]
pub struct Token {
    id: Uuid,
    create_date: NaiveDateTime,
    tag: String
}

impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // 3 is the number of fields in the struct.
        let mut state = serializer.serialize_struct("Token", 3)?;
        state.serialize_field("id", &self.id.to_string())?;
        state.serialize_field("create_date", &self.create_date.to_string())?;
        state.serialize_field("tag", &self.tag)?;
        state.end()
    }
}

#[derive(Clone)]
pub struct AppState {
    pub token_repo: Arc<dyn TokenRepo>,
    pub tag_repo: Arc<dyn TagRepo>,
    pub ip_repo: Arc<dyn IpRepo>,
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
        let uuid = Uuid::new_v4();

        state.token_repo.save_token(&Token {
            id: uuid,
            create_date: NaiveDateTime::default(),
            tag: tag_extracted.clone(),
        });

        let mut uri_new = state.conf.redirect_uri;
        uri_new.push_str("/tag/");
        uri_new.push_str(&tag_extracted);
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
                increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
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
        let response = Response::builder()
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
    if !check_ip(connect_info.ip(), &state.ip_repo, state.conf.max_attempts) {
        increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
        return AppError::Unauthorized.into_response();
    }
    // check if tag exists
    let path = req.uri().path();
    if let Some(tag) = extract_tag_from_path(path) {
        if check_tag(tag.as_str(), state.tag_repo) {
            state.ip_repo.save_or_update(&connect_info.ip(), 0);
            return next.run(req).await;
        } else {
            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
        }
    }

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

fn increment_ip_nb_bad_attempts(ip_addr: &IpAddr, ip_repo: &Arc<dyn IpRepo>) {
    match ip_repo.get(&ip_addr) {
        None => {}
        Some(ip) => {
            ip_repo.save_or_update(ip_addr, ip.nb_bad_attempts + 1);
        }
    }
}

// Placeholder for future token checks
async fn token_guard(
    State(state): State<AppState>,
    ConnectInfo(connect_info): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next
) -> Response {
    if !check_ip(connect_info.ip(), &state.ip_repo, state.conf.max_attempts) {
        increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
        return AppError::Unauthorized.into_response();
    }
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME) {
        if let Ok(header_token_str) = header_token.to_str() {
            if let Ok(token_uuid_requested) = Uuid::parse_str(header_token_str) {
                if let Some(_token) = state.token_repo.get_token(token_uuid_requested) {
                    return next.run(req).await;
                }
            }
        }
    }
    increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
    AppError::Unauthorized.into_response()
}

fn check_tag(tag: &str, tag_repo: Arc<dyn TagRepo>) -> bool {
    tag_repo.get(tag.to_string()).is_some()
}

fn check_ip(ip_addr: IpAddr, ip_repo: &Arc<dyn IpRepo>, max_bad_attempts: u8) -> bool {
    match ip_repo.get(&ip_addr) {
        Some(ip) => {
            println!("{:#?}", ip);
            ip.nb_bad_attempts < max_bad_attempts as u32
        },
        None => true,
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
                let token_opt = state.token_repo.get_token(token_uuid_requested);
                if let Some(token) = token_opt {
                    let mut uri_new = String::from(state.conf.redirect_uri);
                    uri_new.push_str("/tag/");
                    uri_new.push_str(&token.tag);
                    uri_new.push_str("/playlist.m3u8");
                    println!("calling {uri_new}");
                    return match reqwest::get(uri_new).await {
                        Ok(resp) => {
                            Ok(resp.bytes().await.unwrap().into_response())
                        },
                        Err(_) => {
                            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
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
    let headers = req.headers().clone();
    if let Some(header_token) = headers.get(TOKEN_NAME) {
        if let Ok(token_str) = header_token.to_str() {
            if let Ok(token_uuid_requested) = Uuid::parse_str(token_str) {
                let token_opt = state.token_repo.get_token(token_uuid_requested);
                if let Some(token) = token_opt {
                    let mut uri_new = String::from(state.conf.redirect_uri);
                    uri_new.push_str("/tag/");
                    uri_new.push_str(&token.tag);
                    uri_new.push_str("/playlist_");
                    uri_new.push_str(&track_number.to_string());
                    uri_new.push_str(".m3u8");
                    println!("calling {uri_new}");
                    return match reqwest::get(uri_new).await {
                        Ok(resp) => {
                            Ok(resp.bytes().await.unwrap().into_response())
                        },
                        Err(_) => {
                            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
                            Err(AppError::TagNotFound)
                        },
                    }
                }
            }
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
                let token_opt = state.token_repo.get_token(token_uuid_requested);
                if let Some(token) = token_opt {
                    let mut uri_new = String::from(state.conf.redirect_uri);
                    uri_new.push_str("/tag/");
                    uri_new.push_str(&token.tag);
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
                            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
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
        && let Some(token) = state.token_repo.get_token(token_uuid_requested) {

        let mut uri_new = String::from(&state.conf.redirect_uri);
        uri_new.push_str("/tag/");
        uri_new.push_str(&token.tag);
        uri_new.push_str("/playlist.toml");
        println!("checking if there is playlist info at uri: {uri_new}");
        return if let Ok(resp) = reqwest::get(uri_new).await
            && let Ok(text) = resp.text().await
            && text.len() > 0
            && let Ok(playlist_data) = PlaylistData::create_from_toml_text(text.as_str()) {
            Json(playlist_data).into_response()
        } else {
            increment_ip_nb_bad_attempts(&connect_info.ip(), &state.ip_repo);
            AppError::PlaylistNotFound.into_response()
        }
    }
    AppError::Unauthorized.into_response()
}

pub trait TokenRepo: Send + Sync {
    fn get_token(&self, id: Uuid) -> Option<Token>;

    fn save_token(&self, token: &Token);
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryTokenRepo {
    map: Arc<Mutex<HashMap<Uuid, Token>>>,
}

#[derive(Debug, Clone)]
pub struct TokenRepoDB {
    client: redis::Client,
}

impl Default for TokenRepoDB {
    fn default() -> Self {
        let client = redis::Client::open("redis://127.0.0.1/")
            .expect("failed to create a redis client");
        Self { client }
    }
}

impl TokenRepoDB {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self { client: redis::Client::open(redis_url)? })
    }
}

impl TokenRepo for InMemoryTokenRepo {
    fn get_token(&self, id: Uuid) -> Option<Token> {
        self.map.lock().unwrap().get(&id).cloned()
    }

    fn save_token(&self, token: &Token) {
        self.map.lock().unwrap().insert(token.id, token.clone());
    }
}

impl TokenRepo for TokenRepoDB {
    fn get_token(&self, id: Uuid) -> Option<Token> {
        let mut conn = match self.client.get_connection() {
            Ok(c) => c,
            Err(_) => return None,
        };
        let key = format!("token:{}", id);
        let id_s: Option<String> = conn.hget(&key, "id").ok();
        let tag: Option<String> = conn.hget(&key, "tag").ok();
        let create_date_s: Option<String> = conn.hget(&key, "create_date").ok();

        match (id_s, tag, create_date_s) {
            (Some(id_str), Some(tag), Some(cd_str)) => {
                if id_str != id.to_string() {
                    return None;
                }
                let create_date = NaiveDateTime::parse_from_str(&cd_str, "%Y-%m-%d %H:%M:%S").ok()?;
                Some(Token { id, create_date, tag })
            }
            _ => None,
        }
    }

    fn save_token(&self, token: &Token) {
        if let Ok(mut conn) = self.client.get_connection() {
            let key = format!("token:{}", token.id);
            let _: redis::RedisResult<()> = conn.hset_multiple(
                &key,
                &[
                    ("id", token.id.to_string()),
                    ("create_date", token.create_date.format("%Y-%m-%d %H:%M:%S").to_string()),
                    ("tag", token.tag.clone()),
                ],
            );
        }
    }
}

pub trait TagRepo: Send + Sync {
    fn get(&self, tag: String) -> Option<Tag>;

    fn save(&self, tag: &Tag);
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryTagRepo {
    map: Arc<Mutex<HashMap<String, Tag>>>,
}

impl TagRepo for InMemoryTagRepo {
    fn get(&self, name: String) -> Option<Tag> {
        self.map.lock().unwrap().get(&name).cloned()
    }

    fn save(&self, tag: &Tag) {
        self.map.lock().unwrap().insert(tag.id.clone(), tag.clone());
    }
}

#[derive(Debug, Clone)]
pub struct TagRepoDB {
    client: redis::Client,
}

impl Default for TagRepoDB {
    fn default() -> Self {
        let client = create_redis_client();
        Self { client }
    }
}

fn create_redis_client() -> redis::Client {
    redis::Client::open("redis://127.0.0.1/")
        .expect("failed to create a redis client")
}

impl TagRepoDB {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self { client: redis::Client::open(redis_url)? })
    }
}

#[derive(Debug, Clone, new)]
pub struct Tag {
    id: String,
    create_date: NaiveDateTime,
}

impl TagRepo for TagRepoDB {
    fn get(&self, tag: String) -> Option<Tag> {
        let mut conn = match self.client.get_connection() {
            Ok(c) => c,
            Err(_) => return None,
        };
        let key = format!("tag:{}", tag);
        let create_date_s: Option<String> = conn.hget(&key, "create_date").ok();

        match create_date_s {
            Some(cd_str) => {
                let create_date = NaiveDateTime::parse_from_str(&cd_str, "%Y-%m-%d %H:%M:%S").ok()?;
                Some(Tag {
                    id: tag,
                    create_date,
                })
            }
            _ => None,
        }
    }

    fn save(&self, tag: &Tag) {
        if let Ok(mut conn) = self.client.get_connection() {
            let key = format!("tag:{}", tag.id.clone());
            let _: redis::RedisResult<()> = conn.hset_multiple(
                &key,
                &[
                    ("id", tag.id.clone()),
                    ("create_date", tag.create_date.format("%Y-%m-%d %H:%M:%S").to_string())
                ],
            );
        }
    }
}

#[derive(Debug, Clone, new)]
pub struct Ip {
    addr: IpAddr,
    first_seen: NaiveDateTime,
    last_seen: NaiveDateTime,
    nb_bad_attempts: u32,
}

impl Ip {
    pub fn addr(&self) -> &IpAddr {
        &self.addr
    }
    pub fn first_seen(&self) -> &NaiveDateTime {
        &self.first_seen
    }
    pub fn last_seen(&self) -> &NaiveDateTime {
        &self.last_seen
    }
    pub fn nb_bad_attempts(&self) -> &u32 {
        &self.nb_bad_attempts
    }
}

#[derive(Debug, Clone)]
pub struct IpRepoDB {
    client: redis::Client,
}

impl Default for IpRepoDB {
    fn default() -> Self {
        let client = create_redis_client();
        Self { client }
    }
}

impl IpRepoDB {
    pub fn new(redis_url: &str) -> redis::RedisResult<Self> {
        Ok(Self { client: redis::Client::open(redis_url)? })
    }
}

pub trait IpRepo: Send + Sync {
    fn get(&self, ip_addr: &IpAddr) -> Option<Ip>;
    fn save_or_update(&self, ip_addr: &IpAddr, nb_bad_attempts: u32);
}

impl IpRepo for IpRepoDB {
    fn get(&self, ip_addr: &IpAddr) -> Option<Ip> {
        let mut conn = match self.client.get_connection() {
            Ok(c) => c,
            Err(_) => return None,
        };
        let key = format!("ip:{}", ip_addr.to_string());
        let ip_addr_s: Option<String> = conn.hget(&key, "addr").ok();
        let first_seen: Option<String> = conn.hget(&key, "first_seen").ok();
        let last_seen: Option<String> = conn.hget(&key, "last_seen").ok();
        let nb_bad_attempts: Option<String> = conn.hget(&key, "nb_bad_attempts").ok();

        match (ip_addr_s, first_seen, last_seen, nb_bad_attempts) {
            (Some(_ip_addr_str), Some(fs_str), Some(ls_str), Some(nb_bad_attempts_str)) => {
                Some(Ip {
                    addr: ip_addr.clone(),
                    first_seen: NaiveDateTime::parse_from_str(&fs_str, "%Y-%m-%d %H:%M:%S").ok()?,
                    last_seen: NaiveDateTime::parse_from_str(&ls_str, "%Y-%m-%d %H:%M:%S").ok()?,
                    nb_bad_attempts: nb_bad_attempts_str.parse::<u32>().ok()?,
                })
            }
            _ => None,
        }

    }

    fn save_or_update(&self, ip_addr: &IpAddr, nb_bad_attempts: u32) {
        if let Ok(mut conn) = self.client.get_connection() {
            let mut first_seen = NaiveDateTime::default();
            let last_seen= first_seen;
            if let Some(ip) = self.get(ip_addr) {
                first_seen = ip.first_seen;
            }
            let key = format!("ip:{}", ip_addr.to_string());
            let _: redis::RedisResult<()> = conn.hset_multiple(
                &key,
                &[
                    (
                        "addr",
                        ip_addr.to_string()
                    ),
                    (
                        "first_seen",
                        first_seen.format("%Y-%m-%d %H:%M:%S").to_string()
                    ),
                    (
                        "last_seen",
                        last_seen.format("%Y-%m-%d %H:%M:%S").to_string()
                    ),
                    (
                        "nb_bad_attempts",
                        nb_bad_attempts.to_string()
                        )
                ],
            );
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryIpRepo {
    map: Arc<Mutex<HashMap<IpAddr, Ip>>>,
}

impl IpRepo for InMemoryIpRepo {
    fn get(&self, ip_addr: &IpAddr) -> Option<Ip> {
        self.map.lock().unwrap().get(ip_addr).cloned()
    }

    fn save_or_update(&self, ip_addr: &IpAddr, nb_bad_attempts: u32) {
        let mut first_seen = NaiveDateTime::default();
        let last_seen= first_seen;
        if let Some(ip) = self.get(ip_addr) {
            first_seen = ip.first_seen;
        }
        self.map.lock().expect("can't lock mutex").insert(
            ip_addr.clone(),
            Ip::new(
                *ip_addr,
                first_seen,
                last_seen,
                nb_bad_attempts
            ));

    }
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
        Arc<dyn Repo<Playlist>>,
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
        Arc<dyn Repo<Playlist>>,
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