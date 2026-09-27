use crate::admin::auth::AdminUser;
use serde_json::Value;
use sqlx::PgConnection;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Create,
    Update,
    Delete,
}

impl Action {
    fn as_str(&self) -> &'static str {
        match self {
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

/// Records a change. Call it with the transaction that makes the change,
/// so the change and its log are committed (or rolled back) together.
pub async fn record(
    conn: &mut PgConnection,
    admin: &AdminUser,
    entity: &str,
    entity_id: &str,
    action: Action,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query("
INSERT INTO \"audit_log\" (github_login, entity, entity_id, action, before, after)
VALUES ($1, $2, $3, $4, $5, $6)
")
        .bind(&admin.github_login)
        .bind(entity)
        .bind(entity_id)
        .bind(action.as_str())
        .bind(before)
        .bind(after)
        .execute(conn)
        .await?;
    Ok(())
}
