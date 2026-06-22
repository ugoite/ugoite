use anyhow::{anyhow, Result};
use chrono::{Duration, SecondsFormat, Utc};
use opendal::Operator;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::integrity::RealIntegrityProvider;
use crate::{
    asset, entry, form, index, preferences, saved_sql, search, space, sql_session,
    storage::operator_from_uri,
};

pub const MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS: &[&str] = &[
    "admin_user_ids",
    "invitations",
    "member_roles",
    "members",
    "member_invitations",
    "membership_version",
    "owner_user_id",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpacePermission {
    Read,
    WriteContent,
    ManageSpace,
    ManageMembers,
}

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

    pub async fn create_space_for(&self, space_id: &str, actor_user_id: &str) -> Result<()> {
        validate_member_user_id(actor_user_id)?;
        self.create_space(space_id).await?;
        self.bootstrap_admin_member(space_id, actor_user_id).await
    }

    pub async fn ensure_bootstrap_space_for(
        &self,
        space_id: &str,
        actor_user_id: &str,
    ) -> Result<()> {
        validate_member_user_id(actor_user_id)?;
        match self.create_space(space_id).await {
            Ok(()) => self.bootstrap_admin_member(space_id, actor_user_id).await,
            Err(error) if error.to_string().to_lowercase().contains("already exists") => {
                self.bootstrap_admin_member(space_id, actor_user_id).await
            }
            Err(error) => Err(error),
        }
    }

    pub async fn list_space_ids(&self) -> Result<Vec<String>> {
        space::list_spaces(&self.operator).await
    }

    pub async fn list_accessible_space_ids(&self, actor_user_id: &str) -> Result<Vec<String>> {
        let mut accessible = Vec::new();
        for space_id in self.list_space_ids().await? {
            if self
                .has_permission(&space_id, actor_user_id, SpacePermission::Read)
                .await?
            {
                accessible.push(space_id);
            }
        }
        accessible.sort_by(|left, right| {
            let left_reserved = left == "admin-space";
            let right_reserved = right == "admin-space";
            left_reserved
                .cmp(&right_reserved)
                .then_with(|| left.cmp(right))
        });
        Ok(accessible)
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        space::get_space_raw(&self.operator, space_id).await
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        validate_public_space_patch(patch)?;
        space::patch_space(&self.operator, space_id, patch).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        space::get_space(&self.operator, space_id).await.map(|_| ())
    }

    pub async fn require_permission(
        &self,
        space_id: &str,
        actor_user_id: &str,
        permission: SpacePermission,
    ) -> Result<()> {
        if self
            .has_permission(space_id, actor_user_id, permission)
            .await?
        {
            return Ok(());
        }
        Err(anyhow!(
            "Forbidden: user {actor_user_id} does not have {permission:?} permission for space {space_id}"
        ))
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

    pub async fn update_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
        assets: Option<Vec<Value>>,
    ) -> Result<Value> {
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        entry::update_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            markdown,
            parent_revision_id,
            author,
            assets,
            &integrity,
        )
        .await
    }

    pub async fn delete_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        hard_delete: bool,
    ) -> Result<()> {
        entry::delete_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            hard_delete,
        )
        .await
    }

    pub async fn entry_history(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        entry::get_entry_history(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn entry_revision(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
    ) -> Result<Value> {
        entry::get_entry_revision(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
        )
        .await
    }

    pub async fn restore_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
    ) -> Result<Value> {
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        entry::restore_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            author,
            &integrity,
        )
        .await
    }

    pub async fn list_entry_options(
        &self,
        space_id: &str,
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        entry::list_entry_summaries(
            &self.operator,
            &self.workspace_path(space_id),
            form,
            query,
            limit,
        )
        .await
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::SearchResult>> {
        search::search_entries(&self.operator, &self.workspace_path(space_id), query).await
    }

    pub async fn query_entries(&self, space_id: &str, filter: &Value) -> Result<Vec<Value>> {
        index::query_index(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
        )
        .await
    }

    pub async fn execute_sql_query(&self, space_id: &str, sql: &str) -> Result<Vec<Value>> {
        index::execute_sql_query(&self.operator, &self.workspace_path(space_id), sql).await
    }

    pub async fn reindex(&self, space_id: &str) -> Result<()> {
        index::reindex_all(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn space_stats(&self, space_id: &str) -> Result<Value> {
        index::get_space_stats(&self.operator, &self.workspace_path(space_id)).await
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

    pub async fn list_saved_sql(&self, space_id: &str) -> Result<Vec<Value>> {
        saved_sql::list_sql(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn create_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        author: &str,
    ) -> Result<Value> {
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        saved_sql::create_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            author,
            &integrity,
        )
        .await
    }

    pub async fn get_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<Value> {
        saved_sql::get_sql(&self.operator, &self.workspace_path(space_id), sql_id).await
    }

    pub async fn update_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value> {
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        saved_sql::update_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            parent_revision_id,
            author,
            &integrity,
        )
        .await
    }

    pub async fn delete_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<()> {
        saved_sql::delete_sql(&self.operator, &self.workspace_path(space_id), sql_id).await
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
        let expires_in_seconds = validate_invitation_expiry(expires_in_seconds)?;
        let mut settings = self.read_space_settings(space_id).await?;
        if let Some(existing) = settings
            .get("members")
            .and_then(Value::as_object)
            .and_then(|members| members.get(user_id))
        {
            match existing.get("state").and_then(Value::as_str) {
                Some("active") | Some("invited") => {
                    return Err(anyhow!("Member is already active or invited"));
                }
                _ => {}
            }
        }
        let now = now_iso();
        let expires_at = (Utc::now() + Duration::seconds(expires_in_seconds))
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
            .get_mut("member_invitations")
            .and_then(Value::as_object_mut)
            .and_then(|invitations| invitations.get_mut(token))
            .ok_or_else(|| anyhow!("Invitation not found"))?;
        if invitation.get("state").and_then(Value::as_str) != Some("pending") {
            return Err(anyhow!("Invitation is not pending"));
        }
        let now = Utc::now();
        let expires_at = invitation
            .get("expires_at")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Invitation expires_at is missing"))?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| anyhow!("Invitation expires_at is invalid"))?
            .with_timezone(&Utc);
        if expires_at <= now {
            invitation["state"] = Value::String("expired".to_string());
            bump_membership_version(&mut settings);
            self.write_space_settings(space_id, &settings).await?;
            return Err(anyhow!("Invitation has expired"));
        }
        let user_id = invitation
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or(accepted_by)
            .to_string();
        if user_id != accepted_by {
            return Err(anyhow!("Forbidden: invitation belongs to a different user"));
        }
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
        if role != "admin" && is_last_active_admin(&settings, member_user_id) {
            return Err(anyhow!("Cannot demote the last active admin"));
        }
        let member = settings
            .get_mut("members")
            .and_then(Value::as_object_mut)
            .and_then(|members| members.get_mut(member_user_id))
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
        if is_last_active_admin(&settings, member_user_id) {
            return Err(anyhow!("Cannot revoke the last active admin"));
        }
        let member = settings
            .get_mut("members")
            .and_then(Value::as_object_mut)
            .and_then(|members| members.get_mut(member_user_id))
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

    pub async fn bootstrap_admin_member(&self, space_id: &str, actor_user_id: &str) -> Result<()> {
        let mut settings = self.read_space_settings(space_id).await?;
        let members = settings
            .get_mut("members")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| anyhow!("Invalid members format in settings.json"))?;
        if members
            .get(actor_user_id)
            .and_then(|member| member.get("state"))
            .and_then(Value::as_str)
            == Some("active")
        {
            return Ok(());
        }
        let now = now_iso();
        members.insert(
            actor_user_id.to_string(),
            json!({
                "user_id": actor_user_id,
                "role": "admin",
                "state": "active",
                "invited_by": actor_user_id,
                "invited_at": now,
                "activated_at": now,
                "revoked_at": Value::Null,
                "updated_at": now,
            }),
        );
        bump_membership_version(&mut settings);
        self.write_space_settings(space_id, &settings).await
    }

    pub async fn test_storage_connection(&self, uri: &str) -> Result<Value> {
        space::test_storage_connection(uri).await
    }

    async fn has_permission(
        &self,
        space_id: &str,
        actor_user_id: &str,
        permission: SpacePermission,
    ) -> Result<bool> {
        validate_member_user_id(actor_user_id)?;
        let settings = self.read_space_settings(space_id).await?;
        let Some(member) = settings
            .get("members")
            .and_then(Value::as_object)
            .and_then(|members| members.get(actor_user_id))
        else {
            return Ok(false);
        };
        if member.get("state").and_then(Value::as_str) != Some("active") {
            return Ok(false);
        }
        let role = member.get("role").and_then(Value::as_str).unwrap_or("");
        Ok(role_allows(role, permission))
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

pub fn validate_public_space_patch(patch: &Value) -> Result<()> {
    let reserved_keys: Vec<&str> = patch
        .as_object()
        .map(|object| {
            object
                .keys()
                .map(String::as_str)
                .filter(|key| MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(key))
                .collect()
        })
        .unwrap_or_default();

    let mut reserved_keys = reserved_keys;
    if let Some(settings_obj) = patch.get("settings").and_then(Value::as_object) {
        for key in settings_obj.keys().map(String::as_str) {
            if MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(&key) {
                reserved_keys.push(key);
            }
        }
    }
    reserved_keys.sort_unstable();
    reserved_keys.dedup();
    if reserved_keys.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "space patch does not allow membership-managed settings keys: {}. Use the dedicated member commands instead.",
        reserved_keys.join(", ")
    ))
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

fn validate_invitation_expiry(expires_in_seconds: Option<i64>) -> Result<i64> {
    let seconds = expires_in_seconds.unwrap_or(604_800);
    if !(60..=2_592_000).contains(&seconds) {
        return Err(anyhow!(
            "expires_in_seconds must be between 60 and 2592000 seconds"
        ));
    }
    Ok(seconds)
}

fn is_last_active_admin(settings: &Value, member_user_id: &str) -> bool {
    let Some(members) = settings.get("members").and_then(Value::as_object) else {
        return false;
    };
    let Some(target) = members.get(member_user_id) else {
        return false;
    };
    if target.get("state").and_then(Value::as_str) != Some("active")
        || target.get("role").and_then(Value::as_str) != Some("admin")
    {
        return false;
    }
    members
        .iter()
        .filter(|(user_id, member)| {
            user_id.as_str() != member_user_id
                && member.get("state").and_then(Value::as_str) == Some("active")
                && member.get("role").and_then(Value::as_str) == Some("admin")
        })
        .count()
        == 0
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

fn role_allows(role: &str, permission: SpacePermission) -> bool {
    match role {
        "admin" => true,
        "editor" => matches!(
            permission,
            SpacePermission::Read | SpacePermission::WriteContent
        ),
        "viewer" => matches!(permission, SpacePermission::Read),
        _ => false,
    }
}
