use drop_reverse_proxy::{check_drop_file, check_unarchived_drop_files, create_conf_from_toml_file, create_drop_request_from_toml_file, look_for_drop_files_at_path, IpRepo};
use std::fs;
use std::path::Path;

#[test]
fn ip_repo_save_or_update_when_not_exists() {
    let ip_repo = drop_reverse_proxy::InMemoryIpRepo::default();
    ip_repo.save_or_update(&std::net::IpAddr::from([127,0,0,1]), 0);
    assert_eq!(0, *ip_repo.get(&std::net::IpAddr::from([127,0,0,1])).unwrap().nb_bad_attempts());
}

#[test]
fn ip_repo_save_or_update_when_exists() {
    let ip_repo = drop_reverse_proxy::InMemoryIpRepo::default();
    let ip = std::net::IpAddr::from([127,0,0,1]);
    ip_repo.save_or_update(&ip, 0);
    assert_eq!(0, *ip_repo.get(&std::net::IpAddr::from([127,0,0,1])).unwrap().nb_bad_attempts());
}

#[test]
fn ip_repo_save_or_update_when_exists_and_nb_bad_attempts_is_more_than_zero() {
    let ip_repo = drop_reverse_proxy::InMemoryIpRepo::default();
    let ip = std::net::IpAddr::from([127,0,0,1]);
    ip_repo.save_or_update(&ip, 1);
    assert_eq!(1, *ip_repo.get(&std::net::IpAddr::from([127,0,0,1])).unwrap().nb_bad_attempts());
}

#[test]
fn conf_is_ok() {
    let config_result = create_conf_from_toml_file("tests/resources/conf/ok/app.toml");

    println!("config_result: {:#?}", &config_result.as_ref());

    assert!(config_result.is_ok());
    let config = config_result.unwrap();
    assert!(! config.tags().is_empty());
    assert_eq!(["jdznjevb", "xurnxenyoawltkky", "tag3"].to_vec(), *config.tags());
    assert_eq!("http://localhost:8084", config.redirect_uri());
    assert_eq!("127.0.0.1:8000", config.bind_addr());
    assert_eq!(10, config.max_attempts());
    assert!(config.db_conf().is_some());
    assert_eq!("localhost", config.db_conf().unwrap().db_host());
    assert_eq!(5432, config.db_conf().unwrap().db_port());
    assert_eq!("drop_of_culture", config.db_conf().unwrap().db_name());
    assert_eq!("drop_of_culture", config.db_conf().unwrap().db_user());
    assert_eq!("drop_of_culture", config.db_conf().unwrap().db_password());
    assert_eq!(10, config.db_conf().unwrap().db_pool_size());
    assert_eq!(10000, config.db_conf().unwrap().db_timeout());
}

#[test]
fn look_for_drop_files_at_path_returns_empty_list_when_import_path_is_empty() {
    let import_path = "tests/resources/import_path/empty";
    let files = look_for_drop_files_at_path(Path::new(import_path));
    assert!(files.is_empty())
}

#[test]
fn look_for_drop_files_at_path_returns_list_when_import_path_is_empty() {
    let import_path = "tests/resources/import_path/ok";
    let files = look_for_drop_files_at_path(Path::new(import_path));
    assert!(!files.is_empty());
    assert_eq!(files.len(), 2);
    assert!(files.contains(&"tests/resources/import_path/ok/drop_001".to_string()));
    assert!(files.contains(&"tests/resources/import_path/ok/drop_002".to_string()));
}

#[test]
fn look_for_drop_files_at_path_returns_empty_list_when_import_path_does_not_contain_valid_files() {
    let import_path = "tests/resources/import_path/no_valid_files";
    let files = look_for_drop_files_at_path(Path::new(import_path));
    assert!(files.is_empty())
}

#[test]
fn create_drop_from_toml_file_should_return_drop_struct() {
    let drop = create_drop_request_from_toml_file("tests/resources/import_path/untar_drop/ok/drop_ok/drop.txt");
    assert!(drop.is_ok());
    let drop = drop.unwrap();
    assert_eq!("Cool Rasta", drop.artist_name().as_ref().unwrap());
    assert_eq!("Rasta's playlist", drop.artwork_name());
    assert_eq!(3, drop.tracks().len());
    assert!(drop.tracks().contains(&"track001.mp3".to_string()));
    assert!(drop.tracks().contains(&"track002.mp3".to_string()));
    assert!(drop.tracks().contains(&"track003.mp3".to_string()));
}

#[test]
fn check_unarchived_drop_files_should_return_untar_directory_path() {
    let path = "tests/resources/import_path/untar_drop/ok";
    let result = check_unarchived_drop_files(path);
    assert!(result.is_ok());
    assert_eq!("tests/resources/import_path/untar_drop/ok/drop_ok", result.unwrap().0);

    let result = check_unarchived_drop_files(&*(path.to_owned() + "/drop_ok"));
    assert!(result.is_ok());
    assert_eq!("tests/resources/import_path/untar_drop/ok/drop_ok", result.unwrap().0)
}

#[test]
fn check_drop_file_should_return_untar_directory_path() {
    let tar_gz_path = "tests/resources/import_path/correct_tar_gz/drop_ok.tar.gz";
    let result = check_drop_file(&tar_gz_path);
    assert!(result.is_ok());
    let directory_path = result.unwrap().0;
    let path = Path::new(&directory_path);
    assert!(path.is_dir());
    assert!(path.parent().unwrap().file_name().unwrap().to_str().unwrap().starts_with("correct_tar_gz_"));
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "drop_ok");

    // delete created directories
    fs::remove_dir_all(&directory_path).expect("removing directory failed");
}
