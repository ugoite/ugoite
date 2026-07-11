use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use std::collections::BTreeMap;
use ugoite_domain::entry::{EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
use uuid::Uuid;

fn fixture(entry_count: usize, revisions_per_entry: usize) -> (FormDefinition, Vec<EntryRevision>) {
    let form_id = FormId::from(Uuid::from_u128(1));
    let field_id = FieldId::new(100).unwrap();
    let form = FormDefinition {
        id: form_id,
        version: FormVersion::new(1).unwrap(),
        name: "Benchmark".into(),
        description: None,
        fields: vec![FormField {
            id: field_id,
            name: "body".into(),
            field_type: FieldType::String,
            required: true,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }],
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    };
    let mut revisions = Vec::with_capacity(entry_count * revisions_per_entry);
    for entry in 0..entry_count {
        for version in 1..=revisions_per_entry {
            revisions.push(EntryRevision {
                form_id,
                entry_id: EntryId::from(Uuid::from_u128(10_000 + entry as u128)),
                revision_id: RevisionId::from(Uuid::from_u128(1_000_000 + revisions.len() as u128)),
                parent_revision_id: None,
                entry_version: version as u64,
                expected_version: (version > 1).then_some((version - 1) as u64),
                operation: EntryOperation::Upsert,
                committed_at_micros: version as i64,
                author_id: "benchmark".into(),
                form_version: form.version,
                source_kind: "benchmark".into(),
                source_id: None,
                values: BTreeMap::from([(field_id, FieldValue::String("value".into()))]),
                extra_attributes: BTreeMap::new(),
                extension_metadata: BTreeMap::new(),
            });
        }
    }
    (form, revisions)
}

fn current_revision_selection(c: &mut Criterion) {
    for (entries, history) in [(1_000, 1), (1_000, 10), (10_000, 10)] {
        c.bench_function(&format!("current_revision_{entries}x{history}"), |b| {
            b.iter_batched(
                || fixture(entries, history).1,
                |revisions| {
                    let mut current = BTreeMap::new();
                    for revision in revisions {
                        current
                            .entry(revision.entry_id)
                            .and_modify(|known: &mut u64| {
                                *known = (*known).max(revision.entry_version)
                            })
                            .or_insert(revision.entry_version);
                    }
                    current
                },
                BatchSize::LargeInput,
            )
        });
    }
}

criterion_group!(benches, current_revision_selection);
criterion_main!(benches);
