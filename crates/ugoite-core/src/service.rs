use anyhow::{anyhow, Result};
use chrono::{Duration, SecondsFormat, Utc};
use opendal::Operator;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::integrity::RealIntegrityProvider;
use crate::{
    asset, entry, form, preferences, search, space, sql_session, storage::operator_from_uri,
};

#[derive(Clone)]
pub struct UgoiteService {
    operator: Operator,
    root_uri: String,
}

impl UgoiteService {
    pub fn new(root_uri: impl Into<String>) -> Result<Self> {
        let root_uri = root_uri.into();
        let operator = operator_from_uri(&root_uri)?;
        Ok(Self { operator, root_uri })
    }

    pub fn from_operator(operator: Operator, root_uri: impl Into<String>) -> Self {
        Self {
            operator,
            root_uri: root_uri.into(),
        }
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }

    pub fn workspace_path(&self, space_id: &str) -> String {
        format!("spaces/{space_id}")
    }

    pub async fn create_space(&self, space_id: &str) -> Result<()> {
        space::create_space(&self.operator, space_id, &self.root_uri).await
    }

    pub async fn list_space_ids(&self) -> Result<Vec<String>> {
        space::list_spaces(&self.operator).await
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        space::get_space_raw(&self.operator, space_id).await
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        space::patch_space(&self.operator, space_id, patch).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        space::get_space(&self.operator, space_id).await.map(|_| ())
    }

    pub async fn list_forms(&self, space_id: &str) -> Result<Vec<Value>> {
        form::list_forms(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_form(&self, space_id: &str, form_name: &str) -> Result<Value> {
        form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
    }

    pub async fn upsert_form(&self, space_id: &str, form_def: &Value) -> Result<()> {
        form::upsert_form(&self.operator, &self.workspace_path(space_id), form_def).await
    }

    pub async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value> {
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        entry::create_entry(
            &self.operator,
            &workspace,
            entry_id,
            markdown,
            author,
            &integrity,
        )
        .await?;
        entry::get_entry(&self.operator, &workspace, entry_id).await
    }

    pub async fn list_entries(&self, space_id: &str) -> Result<Vec<Value>> {
        entry::list_entries(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_entry(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        entry::get_entry(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        search::search_entries(&self.operator, &self.workspace_path(space_id), query).await
    }

    pub async fn list_assets(&self, space_id: &str) -> Result<Vec<asset::AssetInfo>> {
        asset::list_assets(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn save_asset(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
    ) -> Result<asset::AssetInfo> {
        asset::save_asset(
            &self.operator,
            &self.workspace_path(space_id),
            filename,
            content,
        )
        .await
    }

    pub async fn delete_asset(&self, space_id: &str, asset_id: &str) -> Result<()> {
        asset::delete_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }

    pub async fn get_user_preferences(
        &self,
        user_id: &str,
    ) -> Result<preferences::UserPreferences> {
        preferences::get_user_preferences(&self.operator, user_id).await
    }

    pub async fn patch_user_preferences(
        &self,
        user_id: &str,
        patch: &Value,
    ) -> Result<preferences::UserPreferences> {
        preferences::patch_user_preferences(&self.operator, user_id, patch).await
    }

    pub async fn create_sql_session(&self, space_id: &str, sql: &str) -> Result<Value> {
        sql_session::create_sql_session(&self.operator, &self.workspace_path(space_id), sql).await
    }

    pub async fn get_sql_session(&self, space_id: &str, session_id: &str) -> Result<Value> {
        sql_session::get_sql_session_status(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
        )
        .await
    }

    pub async fn get_sql_session_count(&self, space_id: &str, session_id: &str) -> Result<u64> {
        sql_session::get_sql_session_count(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
        )
        .await
    }

    pub async fn get_sql_session_rows(
        &self,
        space_id: &str,
        session_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        sql_session::get_sql_session_rows(
            &self.operator,
            &self.workspace_path(space_id),
            session_id,
            offset,
            limit,
        )
        .await
    }

    pub async fn list_members(&self, space_id: &str) -> Result<Vec<Value>> {
        let settings = self.read_space_settings(space_id).await?;
        let mut members: Vec<Value> = settings
            .get("members")
            .and_then(Value::as_object)
            .map(|members| members.values().cloned().collect())
            .unwrap_or_default();
        members.sort_by(|left, right| {
            left.get("user_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .cmp(
                    right
                        .get("user_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                )
        });
        Ok(members)
    }

    pub async fn invite_member(
        &self,
        space_id: &str,
        user_id: &str,
        role: &str,
        invited_by: &str,
        expires_in_seconds: Option<i64>,
    ) -> Result<Value> {
        validate_member_user_id(user_id)?;
        validate_assignable_role(role)?;
        let mut settings = self.read_space_settings(space_id).await?;
        let now = now_iso();
        let expires_at = (Utc::now() + Duration::seconds(expires_in_seconds.unwrap_or(604_800)))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let token = Uuid::new_v4().to_string();
        let invitation = json!({
            "token": token,
            "user_id": user_id,
            "role": role,
            "state": "pending",
            "invited_by": invited_by,
            "invited_at": now,
            "expires_at": expires_at,
        });
        let member = json!({
            "user_id": user_id,
            "role": role,
            "state": "invited",
            "invited_by": invited_by,
            "invited_at": now,
            "activated_at": Value::Null,
            "revoked_at": Value::Null,
            "updated_at": now,
        });
        settings["member_invitations"][&token] = invitation.clone();
        settings["members"][user_id] = member;
        bump_membership_version(&mut settings);
        self.write_space_settings(space_id, &settings).await?;
        Ok(json!({
            "invitation": invitation,
            "delivery": {"mode": "manual"},
            "audit_event": {
                "type": "space.member.invited",
                "space_id": space_id,
                "user_id": user_id,
                "actor_user_id": invited_by,
                "created_at": now,
            },
        }))
    }

    pub async fn accept_invitation(
        &self,
        space_id: &str,
        token: &str,
        accepted_by: &str,
    ) -> Result<Value> {
        if token.trim().is_empty() {
            return Err(anyhow!("invitation token is required"));
        }
        let mut settings = self.read_space_settings(space_id).await?;
        let invitation = settings
            .pointer_mut(&format!("/member_invitations/{token}"))
            .ok_or_else(|| anyhow!("Invitation not found"))?;
        if invitation.get("state").and_then(Value::as_str) != Some("pending") {
            return Err(anyhow!("Invitation is not pending"));
        }
        let user_id = invitation
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or(accepted_by)
            .to_string();
        let role = invitation
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("viewer")
            .to_string();
        let invited_by = invitation
            .get("invited_by")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let invited_at = invitation
            .get("invited_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let now = now_iso();
        invitation["state"] = Value::String("accepted".to_string());
        let member = json!({
            "user_id": user_id,
            "role": role,
            "state": "active",
            "invited_by": invited_by,
            "invited_at": invited_at,
            "activated_at": now,
            "revoked_at": Value::Null,
            "updated_at": now,
        });
        settings["members"][&user_id] = member.clone();
        bump_membership_version(&mut settings);
        self.write_space_settings(space_id, &settings).await?;
        Ok(json!({ "member": member }))
    }

    pub async fn update_member_role(
        &self,
        space_id: &str,
        member_user_id: &str,
        role: &str,
    ) -> Result<Value> {
        validate_member_user_id(member_user_id)?;
        validate_assignable_role(role)?;
        let mut settings = self.read_space_settings(space_id).await?;
        let member = settings
            .pointer_mut(&format!("/members/{member_user_id}"))
            .ok_or_else(|| anyhow!("Member not found"))?;
        member["role"] = Value::String(role.to_string());
        member["updated_at"] = Value::String(now_iso());
        let response = member.clone();
        bump_membership_version(&mut settings);
        self.write_space_settings(space_id, &settings).await?;
        Ok(json!({ "member": response }))
    }

    pub async fn revoke_member(&self, space_id: &str, member_user_id: &str) -> Result<Value> {
        validate_member_user_id(member_user_id)?;
        let mut settings = self.read_space_settings(space_id).await?;
        let member = settings
            .pointer_mut(&format!("/members/{member_user_id}"))
            .ok_or_else(|| anyhow!("Member not found"))?;
        let now = now_iso();
        member["state"] = Value::String("revoked".to_string());
        member["revoked_at"] = Value::String(now.clone());
        member["updated_at"] = Value::String(now);
        let response = member.clone();
        bump_membership_version(&mut settings);
        self.write_space_settings(space_id, &settings).await?;
        Ok(json!({ "member": response }))
    }

    async fn read_space_settings(&self, space_id: &str) -> Result<Value> {
        self.ensure_space(space_id).await?;
        let path = format!("{}/settings.json", self.workspace_path(space_id));
        let mut settings = if self.operator.exists(&path).await? {
            serde_json::from_slice(&self.operator.read(&path).await?.to_vec())?
        } else {
            json!({})
        };
        ensure_membership_objects(&mut settings)?;
        Ok(settings)
    }

    async fn write_space_settings(&self, space_id: &str, settings: &Value) -> Result<()> {
        let path = format!("{}/settings.json", self.workspace_path(space_id));
        self.operator
            .write(&path, serde_json::to_vec_pretty(settings)?)
            .await?;
        Ok(())
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ensure_membership_objects(settings: &mut Value) -> Result<()> {
    if !settings.is_object() {
        return Err(anyhow!("space settings must be a JSON object"));
    }
    let object = settings.as_object_mut().expect("checked object");
    object.entry("members").or_insert_with(|| json!({}));
    object
        .entry("member_invitations")
        .or_insert_with(|| json!({}));
    object
        .entry("membership_version")
        .or_insert_with(|| json!(0));
    Ok(())
}

fn bump_membership_version(settings: &mut Value) {
    let next = settings
        .get("membership_version")
        .and_then(Value::as_u64)
        .unwrap_or_default()
        + 1;
    settings["membership_version"] = json!(next);
}

fn validate_member_user_id(user_id: &str) -> Result<()> {
    if user_id.is_empty()
        || user_id.len() > 128
        || !user_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(anyhow!("Invalid member user_id"));
    }
    Ok(())
}

fn validate_assignable_role(role: &str) -> Result<()> {
    match role {
        "admin" | "editor" | "viewer" => Ok(()),
        _ => Err(anyhow!("Invalid member role")),
    }
}
