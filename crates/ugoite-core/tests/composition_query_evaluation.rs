use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use ugoite_core::composition_query::{
    VariableGraphQueryEvaluation, VariableGraphQueryEvaluationDiagnosticCode,
};
use ugoite_core::sql_query::{
    SavedSqlRevisionRef, SqlQueryPage, SqlQueryRequest, SqlResultColumn, SqlResultColumnType,
};
use ugoite_domain::composition::{
    VariableGraph, VariableGraphArgumentBinding, VariableGraphLiteral, VariableGraphNode,
    VariableGraphNodeKind, VariableGraphQueryDefinition, VariableGraphResultType,
    VariableGraphScalarDescriptor, VariableGraphScalarType, VariableGraphValidatedResult,
};
use uuid::Uuid;

fn scalar(scalar_type: VariableGraphScalarType, nullable: bool) -> VariableGraphScalarDescriptor {
    VariableGraphScalarDescriptor {
        scalar_type,
        nullable,
    }
}

fn input(id: &str, scalar_type: VariableGraphScalarType) -> VariableGraphNode {
    VariableGraphNode {
        id: id.to_owned(),
        kind: VariableGraphNodeKind::Input {
            value_type: scalar(scalar_type, false),
            required: true,
            default: None,
        },
    }
}

fn argument(name: &str, variable: &str) -> VariableGraphArgumentBinding {
    VariableGraphArgumentBinding {
        name: name.to_owned(),
        value_type: scalar(VariableGraphScalarType::Integer, false),
        variable: variable.to_owned(),
    }
}

fn scalar_query(
    id: &str,
    definition: VariableGraphQueryDefinition,
    arguments: Vec<VariableGraphArgumentBinding>,
) -> VariableGraphNode {
    VariableGraphNode {
        id: id.to_owned(),
        kind: VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            query: definition,
            arguments,
        },
    }
}

fn table_query(id: &str) -> VariableGraphNode {
    VariableGraphNode {
        id: id.to_owned(),
        kind: VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Table {
                columns: vec![ugoite_domain::composition::VariableGraphColumn {
                    name: "value".to_owned(),
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                }],
            },
            query: VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT value FROM \"G\" ORDER BY value".to_owned(),
            },
            arguments: Vec::new(),
        },
    }
}

fn page(request: &SqlQueryRequest, rows: Vec<Value>) -> SqlQueryPage {
    SqlQueryPage {
        columns: vec!["value".to_owned()],
        rows,
        has_more: false,
        next: None,
        execution_id: request.execution_id,
        request_fingerprint: request.execution_fingerprint().ok(),
        result_schema: Some(vec![SqlResultColumn {
            name: "value".to_owned(),
            column_type: SqlResultColumnType::Integer,
            nullable: false,
        }]),
        exact_integer_rows: None,
    }
}

fn input_values(
    values: impl IntoIterator<Item = (&'static str, i64)>,
) -> BTreeMap<String, VariableGraphLiteral> {
    values
        .into_iter()
        .map(|(id, value)| (id.to_owned(), VariableGraphLiteral::Integer(value)))
        .collect()
}

#[test]
fn only_descriptor_validated_query_values_reach_the_complete_preview() {
    let graph = VariableGraph {
        // Deliberately author the dependent query first; evaluation follows
        // the compiled graph order instead of YAML order.
        variables: vec![
            scalar_query(
                "derived",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $base + $delta AS value".to_owned(),
                },
                vec![argument("base", "base"), argument("delta", "delta")],
            ),
            scalar_query(
                "base",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $threshold AS value".to_owned(),
                },
                vec![argument("threshold", "threshold")],
            ),
            input("delta", VariableGraphScalarType::Integer),
            input("threshold", VariableGraphScalarType::Integer),
        ],
    };
    let execution_id = Uuid::from_u128(0x4461);
    let mut evaluation = VariableGraphQueryEvaluation::new(
        &graph,
        execution_id,
        &input_values([("delta", 3), ("threshold", 4)]),
    )
    .expect("snapshot values match the graph inputs");

    let base_request = evaluation
        .compile_request("base")
        .expect("base query is ready");
    assert_eq!(base_request.execution_id, Some(execution_id));
    assert_eq!(base_request.parameters.get("threshold"), Some(&json!(4)));
    evaluation
        .accept_page(
            "base",
            &base_request,
            &page(&base_request, vec![json!({"value": 4})]),
        )
        .expect("base result matches its scalar descriptor");

    assert_eq!(
        evaluation
            .preview()
            .expect_err("derived result is still missing")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::QueryResultMissing
    );
    let derived_request = evaluation
        .compile_request("derived")
        .expect("validated scalar dependency is ready");
    assert_eq!(derived_request.parameters.get("base"), Some(&json!(4)));
    assert_eq!(derived_request.parameters.get("delta"), Some(&json!(3)));

    let invalid_page = page(
        &derived_request,
        vec![json!({"value": 7}), json!({"value": 8})],
    );
    let diagnostics = evaluation
        .accept_page("derived", &derived_request, &invalid_page)
        .expect_err("a scalar result with multiple rows is rejected");
    assert_eq!(
        diagnostics[0].code,
        VariableGraphQueryEvaluationDiagnosticCode::ResultValidationFailed
    );
    assert!(diagnostics[0]
        .result_diagnostic
        .as_ref()
        .is_some_and(|diagnostic| diagnostic
            .result_diagnostic
            .as_ref()
            .is_some_and(|detail| detail.code.as_str() == "scalar_multiple_rows")));
    assert_eq!(
        evaluation
            .preview()
            .expect_err("invalid result was not retained")[0]
            .variable_id,
        "derived"
    );

    let valid_page = page(&derived_request, vec![json!({"value": 7})]);
    evaluation
        .accept_page("derived", &derived_request, &valid_page)
        .expect("single row satisfies the scalar descriptor");
    let preview = evaluation.preview().expect("all query values are valid");
    assert_eq!(preview.execution_id, execution_id);
    assert_eq!(preview.query_results.len(), 2);
    assert_eq!(
        preview
            .query_results
            .get("derived")
            .map(|value| &value.result),
        Some(&VariableGraphValidatedResult::Scalar {
            value: VariableGraphLiteral::Integer(7),
        })
    );
}

#[test]
fn evaluation_rejects_requests_with_unbound_parameters_or_stale_snapshot_ids() {
    let graph = VariableGraph {
        variables: vec![
            input("threshold", VariableGraphScalarType::Integer),
            scalar_query(
                "total",
                VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                    revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                    column: Some("total".to_owned()),
                },
                vec![argument("floor", "threshold")],
            ),
        ],
    };
    let execution_id = Uuid::from_u128(0x4462);
    let mut evaluation =
        VariableGraphQueryEvaluation::new(&graph, execution_id, &input_values([("threshold", 10)]))
            .expect("input matches saved query argument");
    let request = evaluation
        .compile_request("total")
        .expect("exact Saved SQL request compiles");
    assert_eq!(
        request.saved_sql,
        Some(SavedSqlRevisionRef {
            id: "00000000-0000-0000-0000-000000000001".to_owned(),
            revision_id: "00000000-0000-0000-0000-000000000002".to_owned(),
        })
    );
    assert_eq!(request.parameters.get("floor"), Some(&json!(10)));

    let mut altered_request = request.clone();
    altered_request.parameters = Map::from_iter([("floor".to_owned(), json!(99))]);
    let altered_page = page(&altered_request, vec![json!({"value": 99})]);
    assert_eq!(
        evaluation
            .accept_page("total", &altered_request, &altered_page)
            .expect_err("parameter values must be those bound from this graph snapshot")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::RequestDoesNotMatchGraph
    );

    let mut stale_request = request.clone();
    stale_request.execution_id = Some(Uuid::from_u128(0xdead));
    let stale_page = page(&stale_request, vec![json!({"value": 11})]);
    assert_eq!(
        evaluation
            .accept_page("total", &stale_request, &stale_page)
            .expect_err("page belongs to a different evaluation snapshot")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::RequestSnapshotMismatch
    );

    let pinned_result = SqlQueryPage {
        columns: vec!["total".to_owned()],
        rows: vec![json!({"total": 11})],
        has_more: false,
        next: None,
        execution_id: request.execution_id,
        request_fingerprint: request.execution_fingerprint().ok(),
        result_schema: Some(vec![SqlResultColumn {
            name: "total".to_owned(),
            column_type: SqlResultColumnType::Integer,
            nullable: false,
        }]),
        exact_integer_rows: None,
    };
    evaluation
        .accept_page("total", &request, &pinned_result)
        .expect("pinned revision, selected column, and bound parameter validate");
    assert_eq!(
        evaluation
            .preview()
            .expect("saved query validates")
            .query_results
            .get("total"),
        Some(
            &ugoite_core::composition_query::VariableGraphQueryPreviewValue {
                result: VariableGraphValidatedResult::Scalar {
                    value: VariableGraphLiteral::Integer(11),
                },
                has_more: false,
                next: None,
            }
        )
    );
}

#[test]
fn table_pages_are_validated_and_keep_bounded_continuation_metadata() {
    let graph = VariableGraph {
        variables: vec![table_query("rows")],
    };
    let execution_id = Uuid::from_u128(0x4463);
    let mut evaluation = VariableGraphQueryEvaluation::new(&graph, execution_id, &BTreeMap::new())
        .expect("table query graph is valid");
    let request = evaluation
        .compile_request("rows")
        .expect("table query request compiles");
    let first_page = SqlQueryPage {
        columns: vec!["value".to_owned()],
        rows: vec![json!({"value": 1})],
        has_more: true,
        next: Some("cursor-2".to_owned()),
        execution_id: request.execution_id,
        request_fingerprint: request.execution_fingerprint().ok(),
        result_schema: Some(vec![SqlResultColumn {
            name: "value".to_owned(),
            column_type: SqlResultColumnType::Integer,
            nullable: false,
        }]),
        exact_integer_rows: None,
    };
    let mut malformed_first_page = first_page.clone();
    malformed_first_page.next = None;
    assert_eq!(
        evaluation
            .accept_page("rows", &request, &malformed_first_page)
            .expect_err("has_more requires a next continuation token")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::MalformedPageContinuation
    );
    evaluation
        .accept_page("rows", &request, &first_page)
        .expect("first page satisfies the table descriptor");

    let preview = evaluation
        .preview()
        .expect("validated first page is renderable");
    let table_value = preview.query_results.get("rows").unwrap();
    assert!(table_value.has_more);
    assert_eq!(table_value.next.as_deref(), Some("cursor-2"));
    assert_eq!(
        table_value.result,
        VariableGraphValidatedResult::Table {
            columns: vec![ugoite_domain::composition::VariableGraphColumn {
                name: "value".to_owned(),
                value_type: scalar(VariableGraphScalarType::Integer, false),
            }],
            rows: vec![vec![VariableGraphLiteral::Integer(1)]],
        }
    );

    let second_request = evaluation
        .compile_request("rows")
        .expect("next bounded table page compiles");
    assert_eq!(second_request.continuation.as_deref(), Some("cursor-2"));
    assert_eq!(second_request.execution_id, Some(execution_id));
    assert_eq!(
        evaluation
            .accept_page("rows", &request, &first_page)
            .expect_err("a previous page request cannot be paired with the current cursor")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::RequestDoesNotMatchGraph
    );
    let second_page = SqlQueryPage {
        columns: vec!["value".to_owned()],
        rows: vec![json!({"value": 2})],
        has_more: false,
        next: None,
        execution_id: second_request.execution_id,
        request_fingerprint: second_request.execution_fingerprint().ok(),
        result_schema: Some(vec![SqlResultColumn {
            name: "value".to_owned(),
            column_type: SqlResultColumnType::Integer,
            nullable: false,
        }]),
        exact_integer_rows: None,
    };
    let mut repeated_page = second_page.clone();
    repeated_page.has_more = true;
    repeated_page.next = Some("cursor-2".to_owned());
    assert_eq!(
        evaluation
            .accept_page("rows", &second_request, &repeated_page)
            .expect_err("a previously issued continuation cannot repeat")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::RepeatedPageContinuation
    );
    evaluation
        .accept_page("rows", &second_request, &second_page)
        .expect("second page replaces the prior page in transient state");
    let preview = evaluation
        .preview()
        .expect("latest validated page is renderable");
    let table_value = preview.query_results.get("rows").unwrap();
    assert!(!table_value.has_more);
    assert_eq!(table_value.next, None);
    assert_eq!(
        table_value.result,
        VariableGraphValidatedResult::Table {
            columns: vec![ugoite_domain::composition::VariableGraphColumn {
                name: "value".to_owned(),
                value_type: scalar(VariableGraphScalarType::Integer, false),
            }],
            rows: vec![vec![VariableGraphLiteral::Integer(2)]],
        }
    );
}
