use crate::utils::{create_default_db_config, start_postgres_container};
use drop_reverse_proxy::repository::drop::{Drop, DropRepo};
use drop_reverse_proxy::repository::Repo;

mod utils;

#[tokio::test]
async fn should_insert_data() {
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

    // 3. Initialize schema
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
    .expect("Failed to create table");

    let repo = DropRepo::new(&db_config).await.expect("Failed to create drop repository");

    // 4. Test save_or_update
    let new_drop = Drop::new(0, 1, "Artwork 01".to_string());

    let drop_id = <DropRepo as Repo<Drop>>::save_or_update(&repo, &new_drop).await.expect("Failed to save drop");

    // 5. Test get
    // Since it's the first insert, id should be 1
    let saved_drop = <DropRepo as Repo<Drop>>::get(&repo, drop_id).await.expect("Failed to get drop");
    
    assert_eq!(saved_drop.artwork_id(), 1);
    assert_eq!(saved_drop.id(), drop_id);
    assert_eq!(saved_drop.name(), "Artwork 01");
}