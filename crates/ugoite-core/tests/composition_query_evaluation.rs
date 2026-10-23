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
    VariableGraphNodeKind, VariableGraphQuery, VariableGraphQueryDefinition,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    VariableGraphValidatedResult,
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
            query: VariableGraphQuery {
                definition,
                arguments,
                form_bindings: Vec::new(),
                schema_fingerprint: String::new(),
            },
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
            query: VariableGraphQuery {
                definition: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT value FROM \"G\" ORDER BY value".to_owned(),
                },
                arguments: Vec::new(),
                form_bindings: Vec::new(),
                schema_fingerprint: String::new(),
            },
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
fn omitted_nullable_sql_cells_are_restored_as_null_for_graph_results() {
    let mut query = scalar_query(
        "nullable_total",
        VariableGraphQueryDefinition::SavedSql {
            entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
            revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
            column: Some("value".to_owned()),
        },
        Vec::new(),
    );
    if let VariableGraphNodeKind::Query { returns, .. } = &mut query.kind {
        *returns = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::Float, true),
        };
    }
    let graph = VariableGraph {
        variables: vec![query],
    };
    let execution_id = Uuid::from_u128(0x4464);
    let mut evaluation = VariableGraphQueryEvaluation::new(&graph, execution_id, &BTreeMap::new())
        .expect("nullable scalar graph is valid");
    let request = evaluation
        .compile_request("nullable_total")
        .expect("saved query compiles");
    let mut result = page(&request, vec![json!({})]);
    result.result_schema = Some(vec![SqlResultColumn {
        name: "value".to_owned(),
        column_type: SqlResultColumnType::Float,
        nullable: true,
    }]);

    evaluation
        .accept_page("nullable_total", &request, &result)
        .expect("an omitted nullable JSON field represents SQL NULL");
    assert_eq!(
        evaluation
            .preview()
            .expect("validated null scalar is renderable")
            .query_results
            .get("nullable_total")
            .map(|value| &value.result),
        Some(&VariableGraphValidatedResult::Scalar {
            value: VariableGraphLiteral::Null,
        })
    );
}

#[test]
fn requested_queries_expand_dependencies_and_reject_unselected_or_unknown_nodes() {
    let graph = VariableGraph {
        variables: vec![
            scalar_query(
                "selected",
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
            scalar_query(
                "unrelated",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 99 AS value".to_owned(),
                },
                Vec::new(),
            ),
            input("delta", VariableGraphScalarType::Integer),
            input("threshold", VariableGraphScalarType::Integer),
        ],
    };
    let requested = vec!["selected".to_owned()];
    let evaluation = VariableGraphQueryEvaluation::new_for_queries(
        &graph,
        Uuid::from_u128(0x4436),
        &input_values([("delta", 3), ("threshold", 4)]),
        Some(&requested),
    )
    .expect("selected query and query dependencies are valid");
    assert_eq!(
        evaluation
            .selected_query_ids()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["base", "selected"]
    );
    assert_eq!(
        evaluation
            .compile_request("unrelated")
            .expect_err("unselected query cannot be executed")[0]
            .code,
        VariableGraphQueryEvaluationDiagnosticCode::QueryNotSelected
    );

    let unknown = VariableGraphQueryEvaluation::new_for_queries(
        &graph,
        Uuid::from_u128(0x4437),
        &input_values([("delta", 3), ("threshold", 4)]),
        Some(&["missing".to_owned()]),
    )
    .expect_err("unknown selected IDs fail closed");
    assert_eq!(
        unknown[0].code,
        VariableGraphQueryEvaluationDiagnosticCode::QueryVariableNotFound
    );

    let non_query = VariableGraphQueryEvaluation::new_for_queries(
        &graph,
        Uuid::from_u128(0x4438),
        &input_values([("delta", 3), ("threshold", 4)]),
        Some(&["threshold".to_owned()]),
    )
    .expect_err("only query variables can be selected");
    assert_eq!(
        non_query[0].code,
        VariableGraphQueryEvaluationDiagnosticCode::QueryNodeRequired
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
fn evaluation_accepts_only_the_compiled_table_continuation_request() {
    let graph = VariableGraph {
        variables: vec![table_query("rows")],
    };
    let execution_id = Uuid::from_u128(0x4488);
    let mut evaluation = VariableGraphQueryEvaluation::new(&graph, execution_id, &BTreeMap::new())
        .expect("snapshot has no required input variables");

    let request = evaluation
        .compile_page_request("rows", Some("opaque-cursor"))
        .expect("table continuation can be recompiled statelessly");
    assert_eq!(request.continuation.as_deref(), Some("opaque-cursor"));
    assert_eq!(request.execution_id, Some(execution_id));

    let mut page = page(&request, vec![json!({"value": 9})]);
    page.has_more = true;
    page.next = Some("next-cursor".to_owned());
    evaluation
        .accept_page("rows", &request, &page)
        .expect("page matches the graph query and snapshot");
    let preview = evaluation.preview().expect("table page is validated");
    assert!(matches!(
        preview.query_results.get("rows").map(|value| &value.result),
        Some(VariableGraphValidatedResult::Table { rows, .. }) if rows.len() == 1
    ));

    let mismatched = evaluation
        .compile_page_request("rows", Some("other-cursor"))
        .expect_err("an evaluated table must use the server-returned continuation");
    assert_eq!(
        mismatched[0].code,
        ugoite_core::composition_query::VariableGraphQueryEvaluationDiagnosticCode::RequestDoesNotMatchGraph
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

#[test]
fn selected_query_inputs_do_not_require_unrelated_inputs_but_keep_request_validation() {
    use ugoite_core::composition::bind_variables_for_queries;
    let graph = VariableGraph {
        variables: vec![
            input("b", VariableGraphScalarType::Integer),
            scalar_query(
                "independent",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 1 AS value".to_owned(),
                },
                vec![],
            ),
            scalar_query(
                "dependent",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $b AS value".to_owned(),
                },
                vec![argument("b", "b")],
            ),
            scalar_query(
                "downstream",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $value AS value".to_owned(),
                },
                vec![argument("value", "dependent")],
            ),
        ],
    };
    let requested = vec!["independent".to_owned()];
    for values in [
        BTreeMap::new(),
        BTreeMap::from([("b".to_owned(), json!("invalid"))]),
    ] {
        let bindings = bind_variables_for_queries(&graph.variables, &values, &[], Some(&requested));
        assert!(
            bindings.diagnostics.is_empty(),
            "{:?}",
            bindings.diagnostics
        );
        assert!(!bindings.values.contains_key("b"));
    }
    for inputs in [
        BTreeMap::new(),
        BTreeMap::from([(
            "b".to_owned(),
            VariableGraphLiteral::String("invalid".to_owned()),
        )]),
    ] {
        let state = VariableGraphQueryEvaluation::new_for_queries(
            &graph,
            Uuid::from_u128(0x4490),
            &inputs,
            Some(&requested),
        )
        .expect("unrelated input cannot block independent query");
        assert_eq!(state.selected_query_ids(), requested);
        for selection in [
            None,
            Some(vec!["dependent".to_owned()]),
            Some(vec!["downstream".to_owned()]),
        ] {
            assert!(VariableGraphQueryEvaluation::new_for_queries(
                &graph,
                Uuid::from_u128(0x4490),
                &inputs,
                selection.as_deref()
            )
            .is_err());
        }
    }
    for values in [
        BTreeMap::from([("unknown".to_owned(), json!(1))]),
        BTreeMap::from([("independent".to_owned(), json!(1))]),
    ] {
        assert!(
            !bind_variables_for_queries(&graph.variables, &values, &[], Some(&requested))
                .diagnostics
                .is_empty()
        );
    }
    for unset in [vec!["unknown".to_owned()], vec!["independent".to_owned()]] {
        assert!(!bind_variables_for_queries(
            &graph.variables,
            &BTreeMap::new(),
            &unset,
            Some(&requested)
        )
        .diagnostics
        .is_empty());
    }
    assert!(!bind_variables_for_queries(
        &graph.variables,
        &BTreeMap::new(),
        &[],
        Some(&["unknown".to_owned()])
    )
    .diagnostics
    .is_empty());
}

#[test]
fn selected_query_inputs_apply_complete_domain_value_constraints() {
    use ugoite_core::composition::bind_variables_for_queries;
    use ugoite_domain::composition::{
        MAX_COMPOSITION_VARIABLE_INTEGER, MAX_COMPOSITION_VARIABLE_STRING_BYTES,
    };
    for (kind, json_value, literal) in [
        (
            VariableGraphScalarType::Integer,
            json!(MAX_COMPOSITION_VARIABLE_INTEGER + 1),
            VariableGraphLiteral::Integer(MAX_COMPOSITION_VARIABLE_INTEGER + 1),
        ),
        (
            VariableGraphScalarType::Date,
            json!("2026-02-30"),
            VariableGraphLiteral::Date("2026-02-30".to_owned()),
        ),
        (
            VariableGraphScalarType::String,
            json!("x".repeat(MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1)),
            VariableGraphLiteral::String("x".repeat(MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1)),
        ),
        (
            VariableGraphScalarType::String,
            json!("invalid\u{0000}value"),
            VariableGraphLiteral::String("invalid\u{0000}value".to_owned()),
        ),
    ] {
        let graph = VariableGraph {
            variables: vec![
                input("b", kind),
                scalar_query(
                    "independent",
                    VariableGraphQueryDefinition::InlineSql {
                        sql: "SELECT 1 AS value".to_owned(),
                    },
                    vec![],
                ),
                scalar_query(
                    "dependent",
                    VariableGraphQueryDefinition::InlineSql {
                        sql: "SELECT 1 AS value WHERE $b IS NOT NULL".to_owned(),
                    },
                    vec![argument("b", "b")],
                ),
            ],
        };
        let supplied = BTreeMap::from([("b".to_owned(), json_value)]);
        let inputs = BTreeMap::from([("b".to_owned(), literal)]);
        for selection in [None, Some(vec!["dependent".to_owned()])] {
            assert!(!bind_variables_for_queries(
                &graph.variables,
                &supplied,
                &[],
                selection.as_deref()
            )
            .diagnostics
            .is_empty());
            assert!(VariableGraphQueryEvaluation::new_for_queries(
                &graph,
                Uuid::from_u128(0x4490),
                &inputs,
                selection.as_deref()
            )
            .is_err());
        }
        let selected = vec!["independent".to_owned()];
        let bindings =
            bind_variables_for_queries(&graph.variables, &supplied, &[], Some(&selected));
        assert!(
            bindings.diagnostics.is_empty(),
            "{:?}",
            bindings.diagnostics
        );
        assert!(!bindings.values.contains_key("b"));
        assert!(VariableGraphQueryEvaluation::new_for_queries(
            &graph,
            Uuid::from_u128(0x4490),
            &inputs,
            Some(&selected)
        )
        .is_ok());
    }
}

#[test]
fn unrelated_explicit_unset_does_not_restore_a_declared_default() {
    use ugoite_core::composition::bind_variables_for_queries;
    let mut node = input("b", VariableGraphScalarType::Integer);
    if let VariableGraphNodeKind::Input { default, .. } = &mut node.kind {
        *default = Some(VariableGraphLiteral::Integer(7));
    }
    let graph = VariableGraph {
        variables: vec![
            node,
            scalar_query(
                "independent",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 1 AS value".to_owned(),
                },
                vec![],
            ),
        ],
    };
    let result = bind_variables_for_queries(
        &graph.variables,
        &BTreeMap::new(),
        &["b".to_owned()],
        Some(&["independent".to_owned()]),
    );
    assert!(result.diagnostics.is_empty());
    assert!(!result.values.contains_key("b"));
    let result = bind_variables_for_queries(
        &graph.variables,
        &BTreeMap::from([("b".to_owned(), json!("invalid"))]),
        &[],
        Some(&["independent".to_owned()]),
    );
    assert!(result.diagnostics.is_empty());
    assert!(!result.values.contains_key("b"));
}
