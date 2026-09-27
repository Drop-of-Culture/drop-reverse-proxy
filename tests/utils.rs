use std::process::Command;
use std::thread;
use std::time::Duration;
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;
use drop_reverse_proxy::config::db::DatabaseConfig;

pub async fn start_postgres_container(
    db_name: &str,
    user: &str,
    password: &str
) -> Result<(ContainerAsync<Postgres>, String, u16), Box<dyn std::error::Error>> {

    let postgres_container = Postgres::default()
        .with_db_name(db_name)
        .with_user(user)
        .with_password(password)
        .start().await.expect("Failed to start Postgres container");
    match postgres_container.get_host().await {
        Ok(host) => match postgres_container.get_host_port_ipv4(5432).await {
            Ok(port) => Ok((postgres_container, host.to_string(), port)),
            Err(e) => Err(Box::from(e))
        },
        Err(e) => Err(Box::from(e))
    }
}

pub fn create_default_db_config(host: String, port: u16, db_name: &str, user: &str, password: &str) -> DatabaseConfig {
    DatabaseConfig {
        host: host.clone(),
        port: port as u16,
        database: db_name.to_string(),
        username: user.to_string(),
        password: password.to_string(),
        // not the default one, so tests also cover creating the schema
        schema: "dropofculture".to_string(),
        max_connections: 10,
        min_connections: 1,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(100),
        max_lifetime: Duration::from_secs(1800)
    }
}


// Ensure the container is stopped when the test ends
#[derive(Debug)]
pub struct DockerGuard(pub String);
impl Drop for DockerGuard {
    fn drop(&mut self) {
        println!("docker stop container");
        let _ = Command::new("docker").args(["stop", &self.0]).output();
    }
}

/// Initialize an Apache HTTP/2 (httpd) container similarly to `init_redis_container`.
/// Returns a guard that will stop the container when dropped and the base URL.
pub fn init_apache_http2_container() -> Option<(DockerGuard, String)> {
    // 1) Ensure docker is available
    if Command::new("docker").arg("--version").output().is_err() {
        eprintln!("Skipping Apache http2-backed test: Docker CLI not available");
        return None;
    }

    // 2) Build a custom Apache httpd image using the Dockerfile and configs in tests/resources/apache
    //    This ensures httpd is configured as required by the integration tests.
    let image_tag = format!("drop-rp-tests-httpd:{}", Uuid::new_v4().simple());

    // Build context path and Dockerfile path (relative to project root when running tests)
    let build_out = match Command::new("docker")
        .args([
            "build",
            "-t",
            &image_tag,
            "-f",
            "tests/resources/apache/Dockerfile",
            "tests/resources/apache",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "Skipping Apache http2-backed test: docker build failed: {}\n{}",
                String::from_utf8_lossy(&o.stderr),
                String::from_utf8_lossy(&o.stdout)
            );
            return None;
        }
        Err(e) => {
            eprintln!("Skipping Apache http2-backed test: cannot run docker build: {e}");
            return None;
        }
    };
    let _ = build_out; // silence unused warning if not used in debug output paths

    // 3) Run a disposable container from the freshly built image with published random port
    //    Note: httpd listens on 80/tcp inside the container.
    let run_out = match Command::new("docker")
        .args(["run", "-d", "-P", "--rm", &image_tag]) // exposes 80/tcp
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "Skipping Apache http2-backed test: docker run failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            return None;
        }
        Err(e) => {
            eprintln!("Skipping Apache http2-backed test: cannot run docker: {e}");
            return None;
        }
    };
    let container_id = String::from_utf8_lossy(&run_out.stdout).trim().to_string();

    let guard = DockerGuard(container_id.clone());

    // 4) Obtain the published host port for Apache (container port 80)
    let port_out = match Command::new("docker").args(["port", &container_id, "80/tcp"]).output() {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "Skipping Apache http2-backed test: docker port failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            return None;
        }
        Err(e) => {
            eprintln!("Skipping Apache http2-backed test: cannot get docker port: {e}");
            return None;
        }
    };
    let port_stdout = String::from_utf8_lossy(&port_out.stdout);
    let host_port = match port_stdout
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| l.rsplit(':').next())
        .and_then(|p| p.trim().parse::<u16>().ok())
    {
        Some(p) => p,
        None => {
            eprintln!(
                "Skipping Apache http2-backed test: unable to parse published port from '{}':",
                port_stdout
            );
            return None;
        }
    };
    let base_url = format!("http://127.0.0.1:{}/", host_port);

    // 5) Wait for Apache to be ready by attempting to connect to the TCP port.
    use std::net::TcpStream;
    let mut ready = false;
    for _ in 0..100 { // up to ~10 seconds
        if TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, host_port)).is_ok() {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        eprintln!("Skipping Apache http2-backed test: server in container not ready on {}", base_url);
        None
    } else {
        Some((guard, base_url))
    }
}
