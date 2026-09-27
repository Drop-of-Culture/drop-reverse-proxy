//! Visitor IPs: the proxy refuses an IP once its bad attempts reach `max_attempts`.

use crate::admin::audit::{self, Action};
use crate::admin::auth::AdminUser;
use crate::admin::error::AdminError;
use crate::admin::page::{ListPage, Row, RowAction, redirect_to};
use crate::admin::AdminState;
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;

const ENTITY: &str = "ip";
const LIST_URL: &str = "/ips";
const LIST_LIMIT: i64 = 500;

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/ips", get(list))
        .route("/ips/{addr}/unban", post(unban))
        .route("/ips/{addr}/delete", post(delete))
}

#[derive(sqlx::FromRow, Serialize, Debug)]
struct IpRecord {
    addr: String,
    nb_bad_attempts: i32,
}

#[derive(sqlx::FromRow)]
struct IpLine {
    addr: String,
    nb_bad_attempts: i32,
    first_seen: String,
    last_seen: String,
}

async fn list(admin: AdminUser, State(state): State<AdminState>) -> Result<Response, AdminError> {
    // banned first, then most recently seen
    let ips = sqlx::query_as::<_, IpLine>("
SELECT addr, nb_bad_attempts,
       to_char(first_seen, 'YYYY-MM-DD HH24:MI') AS first_seen,
       to_char(last_seen, 'YYYY-MM-DD HH24:MI') AS last_seen
FROM \"ip\"
ORDER BY nb_bad_attempts >= $1 DESC, last_seen DESC
LIMIT $2
")
        .bind(i32::from(state.max_attempts))
        .bind(LIST_LIMIT)
        .fetch_all(&state.pool)
        .await?;

    let max_attempts = i32::from(state.max_attempts);
    let rows = ips.into_iter().map(|ip| {
        let banned = ip.nb_bad_attempts >= max_attempts;
        let mut actions = Vec::new();
        if ip.nb_bad_attempts > 0 {
            actions.push(RowAction { label: "Unban", url: format!("/ips/{}/unban", ip.addr), danger: false });
        }
        actions.push(RowAction { label: "Delete", url: format!("/ips/{}/delete", ip.addr), danger: true });
        Row {
            cells: vec![
                ip.addr,
                if banned { "banned".to_string() } else { String::new() },
                ip.nb_bad_attempts.to_string(),
                ip.first_seen,
                ip.last_seen,
            ],
            edit_url: None,
            actions,
        }
    }).collect();

    ListPage {
        admin,
        title: "IPs",
        note: Some(format!(
            "An IP is refused from {max_attempts} bad attempts. Unban resets its count to 0 ({LIST_LIMIT} IPs at most)."
        )),
        new_url: None,
        columns: vec!["Address", "Status", "Bad attempts", "First seen", "Last seen"],
        rows,
    }.respond()
}

async fn unban(admin: AdminUser, State(state): State<AdminState>, Path(addr): Path<String>) -> Result<Response, AdminError> {
    let mut tx = state.pool.begin().await?;
    let before = fetch_for_update(&mut tx, &addr).await?;
    let after = sqlx::query_as::<_, IpRecord>("
UPDATE \"ip\" SET nb_bad_attempts = 0 WHERE addr = $1
RETURNING addr, nb_bad_attempts
")
        .bind(&addr)
        .fetch_one(&mut *tx)
        .await?;
    audit::record_change(&mut tx, &admin, ENTITY, &addr, Action::Update, Some(&before), Some(&after)).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn delete(admin: AdminUser, State(state): State<AdminState>, Path(addr): Path<String>) -> Result<Response, AdminError> {
    let mut tx = state.pool.begin().await?;
    let before = fetch_for_update(&mut tx, &addr).await?;
    sqlx::query("DELETE FROM \"ip\" WHERE addr = $1").bind(&addr).execute(&mut *tx).await?;
    audit::record_change(&mut tx, &admin, ENTITY, &addr, Action::Delete, Some(&before), None).await?;
    tx.commit().await?;

    redirect_to(LIST_URL)
}

async fn fetch_for_update(conn: &mut sqlx::PgConnection, addr: &str) -> Result<IpRecord, AdminError> {
    sqlx::query_as::<_, IpRecord>("SELECT addr, nb_bad_attempts FROM \"ip\" WHERE addr = $1 FOR UPDATE")
        .bind(addr)
        .fetch_optional(conn)
        .await?
        .ok_or(AdminError::NotFound)
}
