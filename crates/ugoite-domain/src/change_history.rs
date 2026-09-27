//! Shared read models for append-only Change history.
//!
//! These values are derived from authoritative publications and Entry
//! revisions. They are not a second history store and do not infer a Change
//! from a Run or from user-authored messages.

use crate::change::ChangeDescriptor;
use crate::entry::FieldValue;
use crate::id::{EntryId, FieldId, FormId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A value state in a verified revision comparison.
///
/// `Missing` is distinct from an explicit `FieldValue::Null`. `Unavailable`
/// and `Redacted` carry no value and must not be treated as missing or as a
/// common field change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "value")]
pub enum ComparedFieldValue {
    Missing,
    Value(FieldValue),
    Unavailable,
    Redacted,
}

/// The verified before/after evidence for one field on one affected Entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldChangeEvidence {
    pub field_id: FieldId,
    pub before: ComparedFieldValue,
    pub after: ComparedFieldValue,
}

/// One Entry's proven membership in a Change and its field-level effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryChangeEvidence {
    pub form_id: FormId,
    pub entry_id: EntryId,
    pub fields: Vec<FieldChangeEvidence>,
}

/// A group of Entries with identical, fully available field changes.
///
/// Entries with unavailable or redacted values remain counted as affected,
/// but those values do not contribute to a shared delta group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeFieldGroup {
    pub form_id: FormId,
    pub field_id: FieldId,
    pub before: ComparedFieldValue,
    pub after: ComparedFieldValue,
    pub affected_entry_count: usize,
}

/// A bounded, adapter-neutral summary for a single Change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeHistorySummary {
    /// Number of distinct (Form, Entry) identities with verified membership.
    pub affected_entry_count: usize,
    /// Common field deltas supported by revision evidence.
    pub field_groups: Vec<ChangeFieldGroup>,
}

/// Filters that can be evaluated from the committed Change descriptor alone.
/// Text searches only the optional message; it never guesses at Entry values.
#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChangeHistoryQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_principal_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_after_micros: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_before_micros: Option<i64>,
}

impl ChangeHistoryQuery {
    /// Trim string filters and reject values this protocol cannot search
    /// consistently. The normalized form is also what cursors bind to.
    pub fn normalize(mut self) -> Result<Self, ChangeHistoryQueryError> {
        self.actor_principal_id = normalize_query_text(self.actor_principal_id, 128)?;
        self.run_id = normalize_query_text(self.run_id, 128)?;
        self.text = normalize_query_text(self.text, 256)?;
        self.text = self.text.map(|text| text.to_lowercase());
        if matches!(
            (self.created_after_micros, self.created_before_micros),
            (Some(after), Some(before)) if after > before
        ) {
            return Err(ChangeHistoryQueryError::InvalidTimeRange);
        }
        Ok(self)
    }

    pub fn matches(&self, change: &ChangeDescriptor) -> bool {
        self.actor_principal_id
            .as_deref()
            .is_none_or(|actor| change.actor_principal_id == actor)
            && self.run_id.as_deref().is_none_or(|run_id| {
                change
                    .run_id
                    .as_ref()
                    .is_some_and(|value| value.as_str() == run_id)
            })
            && self.text.as_deref().is_none_or(|query| {
                change
                    .message
                    .as_deref()
                    .is_some_and(|message| message.to_lowercase().contains(query))
            })
            && self
                .created_after_micros
                .is_none_or(|after| change.created_at_micros >= after)
            && self
                .created_before_micros
                .is_none_or(|before| change.created_at_micros <= before)
    }
}

fn normalize_query_text(
    value: Option<String>,
    max_bytes: usize,
) -> Result<Option<String>, ChangeHistoryQueryError> {
    value
        .map(|value| {
            let value = value.trim().to_owned();
            if value.is_empty() || value.chars().count() > max_bytes {
                return Err(ChangeHistoryQueryError::InvalidFilter);
            }
            Ok(value)
        })
        .transpose()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChangeHistoryQueryError {
    InvalidFilter,
    InvalidTimeRange,
}

impl std::fmt::Display for ChangeHistoryQueryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFilter => formatter.write_str("Change history filter is invalid"),
            Self::InvalidTimeRange => {
                formatter.write_str("created_after_micros must not exceed created_before_micros")
            }
        }
    }
}

impl std::error::Error for ChangeHistoryQueryError {}

/// Summarize one Change from verified revision evidence.
///
/// Duplicate references to an Entry are collapsed by identity. Conflicting
/// evidence for the same Entry is rejected rather than guessed. A field group
/// is emitted only for explicit values or missing values; incomplete or
/// redacted observations are intentionally omitted from common-delta claims.
pub fn summarize_change(
    entries: &[EntryChangeEvidence],
) -> Result<ChangeHistorySummary, ChangeSummaryError> {
    let mut unique = BTreeMap::<(FormId, EntryId), &EntryChangeEvidence>::new();
    for entry in entries {
        let key = (entry.form_id, entry.entry_id);
        if let Some(previous) = unique.insert(key, entry) {
            if previous != entry {
                return Err(ChangeSummaryError::ConflictingEntryEvidence);
            }
        }
    }

    let mut groups = BTreeMap::<(FormId, FieldId, String, String), ChangeFieldGroup>::new();
    for entry in unique.values() {
        let mut observed_fields =
            BTreeMap::<FieldId, (ComparedFieldValue, ComparedFieldValue)>::new();
        for field in &entry.fields {
            if let Some(previous) =
                observed_fields.insert(field.field_id, (field.before.clone(), field.after.clone()))
            {
                if previous != (field.before.clone(), field.after.clone()) {
                    return Err(ChangeSummaryError::ConflictingEntryEvidence);
                }
                continue;
            }
            if !is_comparable(&field.before) || !is_comparable(&field.after) {
                continue;
            }
            let before_key = serde_json::to_string(&field.before)
                .map_err(|_| ChangeSummaryError::UnserializableValue)?;
            let after_key = serde_json::to_string(&field.after)
                .map_err(|_| ChangeSummaryError::UnserializableValue)?;
            let key = (entry.form_id, field.field_id, before_key, after_key);
            groups
                .entry(key)
                .and_modify(|group| group.affected_entry_count += 1)
                .or_insert_with(|| ChangeFieldGroup {
                    form_id: entry.form_id,
                    field_id: field.field_id,
                    before: field.before.clone(),
                    after: field.after.clone(),
                    affected_entry_count: 1,
                });
        }
    }

    Ok(ChangeHistorySummary {
        affected_entry_count: unique.len(),
        field_groups: groups.into_values().collect(),
    })
}

fn is_comparable(value: &ComparedFieldValue) -> bool {
    matches!(
        value,
        ComparedFieldValue::Missing | ComparedFieldValue::Value(_)
    )
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChangeSummaryError {
    ConflictingEntryEvidence,
    UnserializableValue,
}

impl std::fmt::Display for ChangeSummaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConflictingEntryEvidence => {
                formatter.write_str("conflicting revision evidence for one Change target")
            }
            Self::UnserializableValue => formatter.write_str("field value cannot be serialized"),
        }
    }
}

impl std::error::Error for ChangeSummaryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::RunId;
    use crate::id::{EntryId, FieldId, FormId};
    use uuid::Uuid;

    fn form(value: u128) -> FormId {
        FormId::from_uuid(Uuid::from_u128(value))
    }

    fn entry(value: u128) -> EntryId {
        EntryId::from_uuid(Uuid::from_u128(value))
    }

    fn field() -> FieldId {
        FieldId::new(100).expect("field id")
    }

    fn changed(
        form_id: FormId,
        entry_id: EntryId,
        before: &str,
        after: &str,
    ) -> EntryChangeEvidence {
        EntryChangeEvidence {
            form_id,
            entry_id,
            fields: vec![FieldChangeEvidence {
                field_id: field(),
                before: ComparedFieldValue::Value(FieldValue::String(before.to_owned())),
                after: ComparedFieldValue::Value(FieldValue::String(after.to_owned())),
            }],
        }
    }

    fn descriptor() -> ChangeDescriptor {
        ChangeDescriptor {
            run_id: Some(RunId::new("run-1").unwrap()),
            actor_principal_id: "actor-1".into(),
            message: Some("Travel 交通費の更新".into()),
            reverts_change_id: None,
            created_at_micros: 1_000,
        }
    }

    #[test]
    fn query_filters_committed_metadata_and_normalizes_text() {
        let query = ChangeHistoryQuery {
            actor_principal_id: Some(" actor-1 ".into()),
            run_id: Some("run-1".into()),
            text: Some("  TRAVEL  ".into()),
            created_after_micros: Some(1_000),
            created_before_micros: Some(1_000),
        }
        .normalize()
        .unwrap();
        assert_eq!(query.text.as_deref(), Some("travel"));
        assert!(query.matches(&descriptor()));

        let other = ChangeHistoryQuery {
            text: Some("lunch".into()),
            ..query
        }
        .normalize()
        .unwrap();
        assert!(!other.matches(&descriptor()));
    }

    #[test]
    fn query_rejects_blank_oversized_and_inverted_filters() {
        assert_eq!(
            ChangeHistoryQuery {
                text: Some("  ".into()),
                ..ChangeHistoryQuery::default()
            }
            .normalize()
            .unwrap_err(),
            ChangeHistoryQueryError::InvalidFilter
        );
        assert_eq!(
            ChangeHistoryQuery {
                text: Some("x".repeat(257)),
                ..ChangeHistoryQuery::default()
            }
            .normalize()
            .unwrap_err(),
            ChangeHistoryQueryError::InvalidFilter
        );
        assert!(ChangeHistoryQuery {
            text: Some("界".repeat(256)),
            ..ChangeHistoryQuery::default()
        }
        .normalize()
        .is_ok());
        assert_eq!(
            ChangeHistoryQuery {
                created_after_micros: Some(2),
                created_before_micros: Some(1),
                ..ChangeHistoryQuery::default()
            }
            .normalize()
            .unwrap_err(),
            ChangeHistoryQueryError::InvalidTimeRange
        );
    }

    #[test]
    fn one_entry_is_one_change_target() {
        let summary = summarize_change(&[changed(form(1), entry(1), "850", "1200")]).unwrap();
        assert_eq!(summary.affected_entry_count, 1);
        assert_eq!(summary.field_groups.len(), 1);
        assert_eq!(summary.field_groups[0].affected_entry_count, 1);
    }

    #[test]
    fn one_hundred_entries_group_only_the_proven_ninety_five_plus_five() {
        let mut evidence = (0..95)
            .map(|id| changed(form(1), entry(id + 1), "交通費", "出張交通費"))
            .collect::<Vec<_>>();
        evidence
            .extend((95..100).map(|id| changed(form(1), entry(id + 1), "交通費", "顧客訪問費")));

        let summary = summarize_change(&evidence).unwrap();
        assert_eq!(summary.affected_entry_count, 100);
        assert_eq!(summary.field_groups.len(), 2);
        assert_eq!(summary.field_groups[0].affected_entry_count, 95);
        assert_eq!(summary.field_groups[1].affected_entry_count, 5);
    }

    #[test]
    fn same_run_does_not_merge_changes_from_different_forms() {
        let first = summarize_change(
            &(0..40)
                .map(|id| changed(form(1), entry(id + 1), "draft", "submitted"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let second = summarize_change(
            &(0..60)
                .map(|id| changed(form(2), entry(id + 101), "draft", "approved"))
                .collect::<Vec<_>>(),
        )
        .unwrap();

        assert_eq!(first.affected_entry_count, 40);
        assert_eq!(second.affected_entry_count, 60);
        assert_ne!(
            first.field_groups[0].form_id,
            second.field_groups[0].form_id
        );
    }

    #[test]
    fn a_committed_prefix_summarizes_only_its_fifty_nine_changes() {
        let confirmed_prefix = (0..59)
            .map(|id| changed(form(1), entry(id + 1), "pending", "saved"))
            .collect::<Vec<_>>();
        let summary = summarize_change(&confirmed_prefix).unwrap();
        assert_eq!(summary.affected_entry_count, 59);
    }

    #[test]
    fn duplicate_entries_collapse_and_conflicting_evidence_is_rejected() {
        let evidence = changed(form(1), entry(1), "old", "new");
        let summary = summarize_change(&[evidence.clone(), evidence]).unwrap();
        assert_eq!(summary.affected_entry_count, 1);

        let error = summarize_change(&[
            changed(form(1), entry(1), "old", "new"),
            changed(form(1), entry(1), "old", "other"),
        ])
        .unwrap_err();
        assert_eq!(error, ChangeSummaryError::ConflictingEntryEvidence);
    }

    #[test]
    fn unavailable_and_redacted_values_do_not_create_inferred_groups() {
        let summary = summarize_change(&[EntryChangeEvidence {
            form_id: form(1),
            entry_id: entry(1),
            fields: vec![FieldChangeEvidence {
                field_id: field(),
                before: ComparedFieldValue::Redacted,
                after: ComparedFieldValue::Unavailable,
            }],
        }])
        .unwrap();
        assert_eq!(summary.affected_entry_count, 1);
        assert!(summary.field_groups.is_empty());
    }

    #[test]
    fn conflicting_visibility_for_a_field_is_rejected() {
        let error = summarize_change(&[EntryChangeEvidence {
            form_id: form(1),
            entry_id: entry(1),
            fields: vec![
                FieldChangeEvidence {
                    field_id: field(),
                    before: ComparedFieldValue::Value(FieldValue::String("old".into())),
                    after: ComparedFieldValue::Value(FieldValue::String("new".into())),
                },
                FieldChangeEvidence {
                    field_id: field(),
                    before: ComparedFieldValue::Redacted,
                    after: ComparedFieldValue::Unavailable,
                },
            ],
        }])
        .unwrap_err();
        assert_eq!(error, ChangeSummaryError::ConflictingEntryEvidence);
    }
}
