use std::collections::BTreeMap;
use ugoite_domain::entry::{
    EntryOperation, EntryRevision, EntryRevisionDraft, FieldValue, RevisionError,
};
use ugoite_domain::form::{
    Compatibility, FieldType, FormChange, FormChangeSet, FormDefinition, FormField, FormVersion,
};
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
use uuid::Uuid;

fn field_id(value: i32) -> FieldId {
    FieldId::new(value).unwrap()
}

fn form() -> FormDefinition {
    FormDefinition {
        id: FormId::from(Uuid::from_u128(1)),
        version: FormVersion::new(1).unwrap(),
        name: "Task".into(),
        description: None,
        fields: vec![FormField {
            id: field_id(100),
            name: "title".into(),
            field_type: FieldType::String,
            required: true,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }],
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    }
}

#[test]
fn field_rename_preserves_stable_id_and_is_compatible() {
    let original = form();
    let changes = FormChangeSet {
        form_id: original.id,
        expected_version: Some(original.version),
        changes: vec![FormChange::RenameField {
            field_id: field_id(100),
            name: "summary".into(),
        }],
    };
    assert_eq!(
        changes.compatibility(&original).unwrap(),
        Compatibility::Compatible
    );
    let evolved = original.apply(&changes).unwrap();
    assert_eq!(evolved.fields[0].id, field_id(100));
    assert_eq!(evolved.fields[0].name, "summary");
    assert_eq!(evolved.version.get(), 2);
}

#[test]
fn required_addition_is_forward_compatible_and_narrowing_is_breaking() {
    let original = form();
    let required = FormChangeSet {
        form_id: original.id,
        expected_version: Some(original.version),
        changes: vec![FormChange::AddField(FormField {
            id: field_id(101),
            name: "done".into(),
            field_type: FieldType::Boolean,
            required: true,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        })],
    };
    assert_eq!(
        required.compatibility(&original).unwrap(),
        Compatibility::Compatible
    );
    let narrowing = FormChangeSet {
        form_id: original.id,
        expected_version: Some(original.version),
        changes: vec![FormChange::ChangeFieldType {
            field_id: field_id(100),
            field_type: FieldType::Boolean,
        }],
    };
    assert_eq!(
        narrowing.compatibility(&original).unwrap(),
        Compatibility::Breaking
    );
}

#[test]
fn revision_validation_enforces_versions_parent_and_tombstones() {
    let form = form();
    let first = EntryRevision {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(2)),
        revision_id: RevisionId::from(Uuid::from_u128(3)),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:alice".into(),
        form_version: form.version,
        source_kind: "api".into(),
        source_id: None,
        entry: Default::default(),
        values: BTreeMap::from([(field_id(100), FieldValue::String("hello".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    };
    first.validate(&form, None).unwrap();
    let mut second = first.clone();
    second.revision_id = RevisionId::from(Uuid::from_u128(4));
    second.parent_revision_id = Some(first.revision_id);
    second.entry_version = 2;
    second.expected_version = Some(1);
    second.validate(&form, Some(&first)).unwrap();
    second.expected_version = Some(0);
    assert_eq!(
        second.validate(&form, Some(&first)),
        Err(RevisionError::VersionConflict)
    );
}

#[test]
fn revision_draft_derives_parent_and_expected_version() {
    let form = form();
    let first = EntryRevisionDraft {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(2)),
        revision_id: RevisionId::from(Uuid::from_u128(3)),
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:alice".into(),
        form_version: form.version,
        source_kind: "wasm".into(),
        source_id: None,
        entry: Default::default(),
        values: BTreeMap::from([(field_id(100), FieldValue::String("first".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
    .build(&form, None)
    .unwrap();
    let second = EntryRevisionDraft {
        form_id: form.id,
        entry_id: first.entry_id,
        revision_id: RevisionId::from(Uuid::from_u128(4)),
        operation: EntryOperation::Upsert,
        committed_at_micros: 2,
        author_id: "human:alice".into(),
        form_version: form.version,
        source_kind: "wasm".into(),
        source_id: None,
        entry: Default::default(),
        values: BTreeMap::from([(field_id(100), FieldValue::String("second".into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
    .build(&form, Some(&first))
    .unwrap();
    assert_eq!(second.parent_revision_id, Some(first.revision_id));
    assert_eq!(second.expected_version, Some(1));
    assert_eq!(second.entry_version, 2);
}

#[test]
fn extra_attributes_follow_form_policy() {
    let mut form = form();
    let revision = EntryRevision {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(20)),
        revision_id: RevisionId::from(Uuid::from_u128(21)),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "human:alice".into(),
        form_version: form.version,
        source_kind: "api".into(),
        source_id: None,
        entry: Default::default(),
        values: BTreeMap::from([(field_id(100), FieldValue::String("body".into()))]),
        extra_attributes: BTreeMap::from([(String::from("priority"), serde_json::json!("high"))]),
        extension_metadata: BTreeMap::new(),
    };
    assert_eq!(
        revision.validate(&form, None),
        Err(RevisionError::ExtraAttributesNotAllowed)
    );
    form.allow_extra_attributes = true;
    revision.validate(&form, None).unwrap();
}
