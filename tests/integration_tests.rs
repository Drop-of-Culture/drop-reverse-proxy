use crate::mock::repository::artist::ArtistRepoMock;
use crate::mock::repository::artwork::ArtworkRepoMock;
use crate::mock::repository::drop::DropRepoMock;
use crate::mock::repository::playlist::PlaylistRepoMock;
use crate::utils::{DockerGuard, init_apache_http2_container};
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use chrono::NaiveDateTime;
use drop_reverse_proxy::service::drop::DropService;
use drop_reverse_proxy::{AppState, Conf, InMemoryIpRepo, InMemoryTagRepo, InMemoryTokenRepo, IpRepo, IpRepoDB, ServiceConf, TOKEN_NAME, Tag, TagRepo, TagRepoDB, Token, TokenRepo, TokenRepoDB, app};
use http_body_util::Empty;
use regex::Regex;
use reqwest::header::SET_COOKIE;
use std::net::{IpAddr, SocketAddr};
use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tower::ServiceExt;
use uuid::Uuid;

mod mock;
pub mod service;
mod utils;

fn init_in_memory_tag_repo() -> InMemoryTagRepo {
    let tag_repo = InMemoryTagRepo::default();
    ["tag1", "tag2", "tag3", "jdznjevb", "xurnxenyoawltkky"].iter()
        .for_each(|t| tag_repo.save(&Tag::new(t.to_string(), NaiveDateTime::default())));
    tag_repo
}

fn init_redis_tag_repo(redis_url: &String) -> Result<TagRepoDB, redis::RedisError> {
    let tag_repo_db = TagRepoDB::new(redis_url)?;
    tag_repo_db.save(&Tag::new("tag1".to_string(), NaiveDateTime::default()));
    tag_repo_db.save(&Tag::new("tag2".to_string(), NaiveDateTime::default()));
    tag_repo_db.save(&Tag::new("tag3".to_string(), NaiveDateTime::default()));
    tag_repo_db.save(&Tag::new("jdznjevb".to_string(), NaiveDateTime::default()));
    tag_repo_db.save(&Tag::new("xurnxenyoawltkky".to_string(), NaiveDateTime::default()));
    Ok(tag_repo_db)
}

#[tokio::test]
async fn get_tag() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = init_in_memory_tag_repo();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(
        base_url, 
        String::from("127.0.0.1:8000"), 
        10, 
        Vec::new(), 
        String::from(""),
        None,
        None
    );
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new())
            )
        ),
    };
    let app = app(app_state.clone());

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server

    let mut req = Request::builder()
        .uri("/tag/xurnxenyoawltkky")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    check_token_in_header_map_is_present_and_uuid(&response.headers());

    let ip_repo_opt = ip_repo.get(&IpAddr::from([127,0,0,1]));
    assert!(ip_repo_opt.is_some());
    assert_eq!(0, *ip_repo_opt.unwrap().nb_bad_attempts());
}

fn check_token_in_header_map_is_present_and_uuid(header_map: &HeaderMap) -> Uuid {
    assert!(header_map.get(SET_COOKIE).is_some());
    let set_cookie_header_value = header_map.get(SET_COOKIE).unwrap();
    assert!(! set_cookie_header_value.is_empty());
    let set_cookie = set_cookie_header_value.to_str().unwrap();
    let re_str = format!(r"{}=(.+)(?:,|\b)", TOKEN_NAME);
    let re = Regex::new(&re_str).unwrap();
    let captures = re.captures(set_cookie);
    assert!(captures.is_some());
    let caps = captures.unwrap();
    assert!(caps.get(1).is_some());
    let token = caps.get(1).unwrap().as_str().to_string();
    let token_as_uuid = Uuid::from_str(token.as_str());
    assert!(token_as_uuid.is_ok());
    token_as_uuid.unwrap()
}

#[tokio::test]
async fn get_tag_error() {
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state);

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server.
    let mut req = Request::builder()
        .uri("/tag/tagUnknown")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn tag_not_in_list_returns_500_and_no_token_header() {
    // Arrange: use in-memory repo
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state);

    // Act: request a tag that is not in the allowed list
    let mut req = Request::builder()
        .uri("/tag/not-allowed-tag")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    // Assert: 500 returned by the guard/handler and no token header set
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.headers().get(TOKEN_NAME).is_none());
}

#[tokio::test]
async fn save_and_get_token_from_repo() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    // Arrange: app with in-memory repo
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = init_in_memory_tag_repo();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(base_url, String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // Act: call the endpoint that saves a token with tag2
    let mut req = Request::builder()
        .uri("/tag/jdznjevb")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Extract token id from header
    let token_id = check_token_in_header_map_is_present_and_uuid(&response.headers());

    // Assert: repo contains the saved token and fields match
    let token = app_state.token_repo.get_token(token_id).expect("token not found in repo");

    // Serialize to inspect private fields
    let json = serde_json::to_value(&token).unwrap();
    assert_eq!(json.get("id").and_then(|v| v.as_str()), Some(token_id.to_string().as_str()));
    assert_eq!(json.get("tag").and_then(|v| v.as_str()), Some("jdznjevb"));
    let ip_repo_opt = ip_repo.get(&IpAddr::from([127,0,0,1]));
    assert!(ip_repo_opt.is_some());
    assert_eq!(0, *ip_repo_opt.unwrap().nb_bad_attempts());
}

fn init_redis_container() -> Option<(DockerGuard, String)> {
    // Try to launch a Redis container using Docker. If Docker is unavailable, skip the test.
    // 1) Ensure docker is available
    if Command::new("docker").arg("--version").output().is_err() {
        eprintln!("Skipping Redis-backed test: Docker CLI not available");
        return None;
    }

    // 2) Run a disposable Redis container with published random port
    let run_out = match Command::new("docker")
        .args(["run", "-d", "-P", "--rm", "redis:7-alpine"]) // lightweight image
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("Skipping Redis-backed test: docker run failed: {}", String::from_utf8_lossy(&o.stderr));
            return None;
        }
        Err(e) => {
            eprintln!("Skipping Redis-backed test: cannot run docker: {e}");
            return None;
        }
    };
    let container_id = String::from_utf8_lossy(&run_out.stdout).trim().to_string();

    let guard = DockerGuard(container_id.clone());

    // 3) Obtain the published host port for Redis (container port 6379)
    let port_out = match Command::new("docker").args(["port", &container_id, "6379/tcp"]).output() {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!("Skipping Redis-backed test: docker port failed: {}", String::from_utf8_lossy(&o.stderr));
            return None;
        }
        Err(e) => {
            eprintln!("Skipping Redis-backed test: cannot get docker port: {e}");
            return None;
        }
    };
    let port_stdout = String::from_utf8_lossy(&port_out.stdout);
    // docker may print multiple lines (IPv4 and IPv6). Take first non-empty line and parse last ':'
    let host_port = match port_stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.rsplit(':').next())
        .and_then(|p| p.trim().parse::<u16>().ok())
    {
        Some(p) => p,
        None => {
            eprintln!("Skipping Redis-backed test: unable to parse published port from '{}':", port_stdout);
            return None;
        }
    };
    let redis_url = format!("redis://127.0.0.1:{}/", host_port);

    // 4) Wait for Redis to be ready (retry for a short period)
    let mut ready = false;
    for _ in 0..30 { // ~3 seconds
        if let Ok(client) = redis::Client::open(redis_url.as_str()) {
            if client.get_connection().is_ok() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        eprintln!("Skipping Redis-backed test: Redis in container not ready on {}", redis_url);
    } else {
        return Some((guard, redis_url));
    }
    None
}

#[tokio::test]
async fn apache_container_is_ok() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    let apache_url = format!("{base_url}");
    match reqwest::get(apache_url).await {
        Ok(resp) => {
            assert!(resp.status().is_success())
        },
        Err(err) => panic!("{:#?}", err)
    }

    let apache_url = format!("{base_url}tag/jdznjevb/out000.ts");
    match reqwest::get(apache_url).await {
        Ok(resp) => {
            assert!(resp.status().is_success())
        },
        Err(err) => panic!("{:#?}", err)
    }
}

#[tokio::test]
async fn save_and_get_token_from_db() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    // Arrange: app with Redis-backed repo pointing to the container
    let token_repo = TokenRepoDB::new(&redis_url).expect("failed to create TokenRepoDB");
    let tag_repo = init_redis_tag_repo(&redis_url).expect("failed to init TagRepoDB");
    let ip_repo = IpRepoDB::new(&redis_url).expect("failed to create IpRepoDB");
    ip_repo.save_or_update(&IpAddr::from([127,0,0,1]), 0);
    let conf = Conf::new(base_url, String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // Act: call the endpoint that saves a token with tag2
    let mut req = Request::builder()
        .uri("/tag/jdznjevb")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    
    // Extract token id from header
    let token_id = check_token_in_header_map_is_present_and_uuid(&response.headers());

    // Assert: repo contains the saved token and fields match
    let token = app_state
        .token_repo
        .get_token(token_id)
        .expect("token not found in db repo");

    // Serialize to inspect private fields
    let json = serde_json::to_value(&token).unwrap();
    assert_eq!(json.get("id").and_then(|v| v.as_str()), Some(token_id.to_string().as_str()));
    assert_eq!(json.get("tag").and_then(|v| v.as_str()), Some("jdznjevb"));

    let ip_repo_opt = ip_repo.get(&IpAddr::from([127,0,0,1]));
    assert!(ip_repo_opt.is_some());
    assert_eq!(0, *ip_repo_opt.unwrap().nb_bad_attempts());
}

#[tokio::test]
async fn get_tag_should_return_500_when_ip_max_attempts_reached() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();

    // Arrange: app with Redis-backed repo pointing to the container
    let token_repo = TokenRepoDB::new(&redis_url).expect("failed to create TokenRepoDB");
    let tag_repo = init_redis_tag_repo(&redis_url).expect("failed to init TagRepoDB");
    let ip_repo = IpRepoDB::new(&redis_url).expect("failed to create IpRepoDB");
    ip_repo.save_or_update(&IpAddr::from([127,0,0,1]), 10);
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // Act: call the endpoint that saves a token with tag2
    let mut req = Request::builder()
        .uri("/tag/tag2")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Extract token id from header
    let headers = response.headers();
    assert!(headers.get(TOKEN_NAME).is_none());
}

#[tokio::test]
async fn get_play_is_authorized_token() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let token_uuid_valid = Uuid::new_v4();
    let tag_ok = "tag1";
    let token = Token::new(
        token_uuid_valid,
        NaiveDateTime::default(),
        tag_ok.to_string()
    );
    token_repo.save_token(&token);
    let conf = Conf::new(base_url, String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server.
    let mut req = Request::builder()
        .header(TOKEN_NAME, token_uuid_valid.to_string())
        .uri("/play")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_play_is_not_authorized_token() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(base_url, String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),   
    };
    let app = app(app_state.clone());

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server.
    let mut req = Request::builder()
        .header(TOKEN_NAME, "dummy token")
        .uri("/play")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_play_is_not_authorized_token_when_random_path_and_no_token_header() {
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = init_in_memory_tag_repo();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    for path in [
        "/",
        "/random",
        "/random/path",
        "/random/path?query=param",
        "/random/path#fragment"] {
        // `Router` implements `tower::Service<Request<Body>>` so we can
        // call it like any tower service, no need to run an HTTP server.
        let mut req = Request::builder()
            .uri(path)
            .body(Empty::new())
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
        let response = app.clone().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn get_play_is_authorized_token_and_ip_is_allowed() {
    let ( _guard, base_url) = init_apache_http2_container()
        .expect("no apache http container launched");

    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let ip_addr = [127,0,0,1];
    ip_repo.save_or_update(&IpAddr::from(ip_addr), 5);
    let token_uuid_valid = Uuid::new_v4();
    let tag_ok = "tag1";
    let token = Token::new(
        token_uuid_valid,
        NaiveDateTime::default(),
        tag_ok.to_string()
    );
    token_repo.save_token(&token);
    let conf = Conf::new(base_url, String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server.
    let mut req = Request::builder()
        .header(TOKEN_NAME, token_uuid_valid.to_string())
        .uri("/play")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127,0,0,1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_play_is_authorized_token_and_ip_is_not_allowed() {
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let ip_addr = [127,0,0,1];
    ip_repo.save_or_update(&IpAddr::from(ip_addr), 10);
    let token_uuid_valid = Uuid::new_v4();
    let tag_ok = "tag1";
    let token = Token::new(
        token_uuid_valid,
        NaiveDateTime::default(),
        tag_ok.to_string()
    );
    token_repo.save_token(&token);
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());

    // `Router` implements `tower::Service<Request<Body>>` so we can
    // call it like any tower service, no need to run an HTTP server.
    let mut req = Request::builder()
        .header(TOKEN_NAME, token_uuid_valid.to_string())
        .uri("/play")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from((ip_addr, 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// IpRepoDB tests
#[test]
fn ip_repo_save_or_update_when_not_exists() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();
    let ip_repo = IpRepoDB::new(redis_url.as_str()).expect("failed to create IpRepoDB");
    ip_repo.save_or_update(&IpAddr::from([127,0,0,1]), 0);
    assert_eq!(0, *ip_repo.get(&IpAddr::from([127,0,0,1])).unwrap().nb_bad_attempts());
}

#[test]
fn ip_repo_save_or_update_when_exists() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();
    let ip_repo = IpRepoDB::new(redis_url.as_str()).expect("failed to create IpRepoDB");
    let ip = std::net::IpAddr::from([127,0,0,1]);
    ip_repo.save_or_update(&ip, 0);
    assert_eq!(0, *ip_repo.get(&ip).unwrap().nb_bad_attempts());
}

#[test]
fn ip_repo_save_or_update_when_exists_and_nb_bad_attempts_is_more_than_zero() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();
    let ip_repo = IpRepoDB::new(redis_url.as_str()).expect("failed to create IpRepoDB");
    let ip = std::net::IpAddr::from([127,0,0,1]);
    ip_repo.save_or_update(&ip, 1);
    assert_eq!(1, *ip_repo.get(&ip).unwrap().nb_bad_attempts());
}

#[tokio::test]
async fn get_play_is_not_authorized_token_when_no_token() {
    let (_docker_guard, redis_url) = init_redis_container().unwrap();
    let (_docker_guard_apache_http2, apache_url) = init_apache_http2_container().unwrap();

    // Arrange: app with Redis-backed repo pointing to the container
    let tag_ok = "tag1";
    let token_uuid_valid = Uuid::new_v4();
    let token = Token::new(
        token_uuid_valid,
        NaiveDateTime::default(),
        tag_ok.to_string()
    );
    let token_repo = TokenRepoDB::new(&redis_url).expect("failed to create TokenRepoDB");
    token_repo.save_token(&token);
    let tag_repo = init_redis_tag_repo(&redis_url).expect("failed to init TagRepoDB");
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state.clone());
    
    let ip_addr = [127,0,0,1];
    let mut req = Request::builder()
        .uri("/play")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from((ip_addr, 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn drop_import_ok() {
    // Arrange: use in-memory repo
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from("tests/resources/import_path"), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state);

    let mut req = Request::builder()
        .uri("/drop/import")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(StatusCode::OK, response.status());
}

#[tokio::test]
async fn tag_import_returns_not_found_when_called_with_ip_not_accepted() {
    // Arrange: use in-memory repo
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    let ip_repo = InMemoryIpRepo::default();
    let conf = Conf::new(String::from(""), String::from("127.0.0.1:8000"), 10, Vec::new(), String::from(""), None, None);
    let app_state = AppState {
        token_repo: Arc::new(token_repo.clone()),
        tag_repo: Arc::new(tag_repo.clone()),
        ip_repo: Arc::new(ip_repo.clone()),
        conf,
        entity_repositories: Vec::new(),
        service_conf: ServiceConf::new(
            DropService::new(
                Arc::new(DropRepoMock::new()),
                Arc::new(ArtistRepoMock::new()),
                Arc::new(ArtworkRepoMock::new()),
            )
        ),
    };
    let app = app(app_state);

    let mut req = Request::builder()
        .uri("/drop/import")
        .body(Empty::new())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([12, 0, 0, 1], 12345))));
    let response = app.oneshot(req).await.unwrap();

    assert_eq!(StatusCode::NOT_FOUND, response.status());
}
