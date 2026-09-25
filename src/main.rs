use drop_reverse_proxy::config::db::DatabaseConfig;
use drop_reverse_proxy::repository::artist::ArtistRepo;
use drop_reverse_proxy::repository::artwork::ArtworkRepo;
use drop_reverse_proxy::repository::drop::DropRepo;
use drop_reverse_proxy::repository::tag::TagRepo;
use drop_reverse_proxy::repository::ip::IpRepo;
use drop_reverse_proxy::repository::token::TokenRepo;
use drop_reverse_proxy::repository::{Repo, RepoByName};
use drop_reverse_proxy::service::drop::DropService;
use drop_reverse_proxy::{AppState, ServiceConf, app, create_conf_from_toml_file};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use drop_reverse_proxy::repository::redirect::RedirectRepo;

#[tokio::main]
async fn main() {
    let conf = create_conf_from_toml_file("app.toml")
        .expect("can't load conf from toml file");
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

    let migration_pool = drop_reverse_proxy::config::db::create_pool(&db_config)
        .await
        .expect("can't connect to database to run migrations");
    drop_reverse_proxy::config::db::run_migrations(&migration_pool)
        .await
        .expect("failed to run database migrations");
    migration_pool.close().await;
    
    let db_pool = drop_reverse_proxy::config::db::create_pool(&db_config)
        .await
        .expect("can't connect to database");
    
    /*["jdznjevb", "xurnxenyoawltkky", "tag3", "playlist", "simpleredirect"].iter()
        .for_each(|t| tag_repo.save(&Tag::new(t.to_string(), NaiveDateTime::default())));*/
    //tag_repo.save(&drop_reverse_proxy::Tag::new("tag1".to_string(), chrono::NaiveDateTime::default()));

    let listener = tokio::net::TcpListener::bind(conf.bind_addr()).await.unwrap();

    if let Ok(drop_repository) = DropRepo::new(&db_config).await
        && let Ok(artwork_repository) = ArtworkRepo::new(&db_config).await
        && let Ok(artist_repository) = ArtistRepo::new(&db_config).await
        && let Ok(tag_repo) = TagRepo::new(&db_config).await
        && let Ok(token_repo) = TokenRepo::from_pool(db_pool.clone())
        && let Ok(ip_repo) = IpRepo::from_pool(db_pool.clone())
        && let Ok(redirect_repo) = RedirectRepo::new(&db_config).await {
        println!("Database connection successful");

        let drop_service = DropService::new(
            Arc::new(drop_repository) as Arc<dyn Repo<drop_reverse_proxy::repository::drop::Drop>>,
            Arc::new(artist_repository) as Arc<dyn RepoByName<drop_reverse_proxy::repository::artist::Artist>>,
            Arc::new(artwork_repository) as Arc<dyn Repo<drop_reverse_proxy::repository::artwork::Artwork>>,
            Arc::new(redirect_repo) as Arc<dyn Repo<drop_reverse_proxy::repository::redirect::Redirect>>,
        );
        let app_state = AppState {
            token_repo: Arc::new(token_repo.clone()),
            tag_repo: Arc::new(tag_repo.clone()),
            ip_repo: Arc::new(ip_repo.clone()),
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
