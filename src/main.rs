use chrono::NaiveDateTime;
use drop_reverse_proxy::config::db::DatabaseConfig;
use drop_reverse_proxy::repository::drop::DropRepo;
use drop_reverse_proxy::service::drop::DropService;
use drop_reverse_proxy::{app, create_conf_from_toml_file, AppState, InMemoryIpRepo, InMemoryTagRepo, InMemoryTokenRepo, ServiceConf, Tag, TagRepo};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use drop_reverse_proxy::repository::{Repo, RepoByName};
use drop_reverse_proxy::repository::artist::ArtistRepo;
use drop_reverse_proxy::repository::playlist::PlaylistRepo;

#[tokio::main]
async fn main() {
    let conf = create_conf_from_toml_file("app.toml")
        .expect("can't load conf from toml file");

    let listener = tokio::net::TcpListener::bind(conf.bind_addr()).await.unwrap();
    let token_repo = InMemoryTokenRepo::default();
    let tag_repo = InMemoryTagRepo::default();
    ["jdznjevb", "xurnxenyoawltkky", "tag3", "playlist"].iter()
        .for_each(|t| tag_repo.save(&Tag::new(t.to_string(), NaiveDateTime::default())));
    let ip_repo = InMemoryIpRepo::default();
    //tag_repo.save(&drop_reverse_proxy::Tag::new("tag1".to_string(), chrono::NaiveDateTime::default()));

    let db_conf = conf.db_conf().expect("db_conf not found in app.toml");
    let db_config = DatabaseConfig {
        host: db_conf.db_host().to_string(),
        port: db_conf.db_port(),
        database: db_conf.db_name().to_string(),
        username: db_conf.db_user().to_string(),
        password: db_conf.db_password().to_string(),
        max_connections: 10,
        min_connections: 1,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(100),
        max_lifetime: Duration::from_secs(1800)
    };

    if let Ok(drop_repository) = DropRepo::new(&db_config).await
        && let Ok(playlist_repository) = PlaylistRepo::new(&db_config).await
        && let Ok(artist_repository) = ArtistRepo::new(&db_config).await {
        println!("Database connection successful");

        let drop_service = DropService::new(
            Arc::new(drop_repository) as Arc<dyn Repo<drop_reverse_proxy::repository::drop::Drop>>,
            Arc::new(artist_repository) as Arc<dyn RepoByName<drop_reverse_proxy::repository::artist::Artist>>,
            Arc::new(playlist_repository) as Arc<dyn Repo<drop_reverse_proxy::repository::playlist::Playlist>>,
        );
        let app_state = AppState {
            token_repo: Arc::new(token_repo.clone()),
            tag_repo: Arc::new(tag_repo.clone()),
            ip_repo: Arc::new(ip_repo),
            conf,
            entity_repositories: Vec::new(),
            service_conf: ServiceConf::new(drop_service),
        };
        axum::serve(
            listener,
            app(app_state).into_make_service_with_connect_info::<SocketAddr>()
        ).await.unwrap();
    } else {
        panic!("Database connection failed");
    }
}
