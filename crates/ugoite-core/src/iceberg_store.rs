use anyhow::{anyhow, Context, Result};
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use iceberg::spec::{ListType, NestedField, Schema, StructType, Type, UnboundPartitionSpec};
use iceberg::spec::{PrimitiveType, SortOrder};
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableCreation, TableIdent};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use uuid::Uuid;

const FORM_DEF_PROP: &str = "ugoite.form_definition";
const FORM_VERSION_PROP: &str = "ugoite.form_version";
const CATALOG_POINTERS_FILE: &str = "forms/catalog-pointers.v1.json";
const CATALOG_INSTANCE_FILE: &str = "forms/catalog-instance-id";

#[derive(Debug, Default, Serialize, Deserialize)]
struct CatalogPointers {
    version: u32,
    tables: Vec<CatalogTablePointer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CatalogTablePointer {
    namespace: Vec<String>,
    table: String,
    metadata_location: String,
    #[serde(default)]
    form_id: Option<String>,
    #[serde(default)]
    form_name: Option<String>,
}

static CATALOG_CACHE: OnceLock<Mutex<HashMap<String, Arc<dyn Catalog>>>> = OnceLock::new();

fn catalog_cache() -> &'static Mutex<HashMap<String, Arc<dyn Catalog>>> {
    CATALOG_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn scheme_to_uri_prefix(scheme: &str) -> &'static str {
    match scheme {
        "fs" | "file" => "file://",
        "memory" => "memory://",
        "s3" => "s3://",
        "gcs" | "gs" => "gs://",
        "oss" => "oss://",
        "azdls" | "abfs" => "abfs://",
        _ => "fs://",
    }
}

fn normalize_root(root: &str) -> String {
    let trimmed = root.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn warehouse_uri(op: &Operator, ws_path: &str) -> Result<String> {
    let scheme = op.info().scheme();
    let prefix = scheme_to_uri_prefix(scheme);
    let root = normalize_root(op.info().root().as_str());
    let ws_path = ws_path.trim_start_matches('/');
    let warehouse_path = format!("{}/{}", root, ws_path);
    Ok(format!("{}{}{}", prefix, warehouse_path, "/forms"))
}

async fn catalog_for_space(op: &Operator, ws_path: &str) -> Result<Arc<dyn Catalog>> {
    let warehouse = warehouse_uri(op, ws_path)?;
    let rest_catalog_uri = std::env::var("UGOITE_ICEBERG_CATALOG_URI")
        .ok()
        .filter(|uri| !uri.trim().is_empty());
    let cache_key = if let Some(uri) = &rest_catalog_uri {
        format!("rest:{uri}#{warehouse}")
    } else {
        let instance_path = format!(
            "{}/{}",
            ws_path.trim_end_matches('/'),
            CATALOG_INSTANCE_FILE
        );
        let instance_id = if op.exists(&instance_path).await? {
            String::from_utf8(op.read(&instance_path).await?.to_vec())?
        } else {
            let value = Uuid::new_v4().to_string();
            op.write(&instance_path, value.as_bytes().to_vec()).await?;
            value
        };
        format!("{warehouse}#{instance_id}")
    };
    if let Some(catalog) = catalog_cache()
        .lock()
        .map_err(|_| anyhow!("catalog cache lock poisoned"))?
        .get(&cache_key)
        .cloned()
    {
        return Ok(catalog);
    }
    let use_rest_catalog = rest_catalog_uri.is_some();
    let catalog: Arc<dyn Catalog> = if let Some(uri) = rest_catalog_uri {
        ugoite_iceberg::IcebergWorkspace::rest_catalog(&uri, &warehouse, []).await?
    } else {
        let mut props = HashMap::new();
        props.insert(MEMORY_CATALOG_WAREHOUSE.to_string(), warehouse.clone());
        Arc::new(
            MemoryCatalogBuilder::default()
                .load("ugoite", props)
                .await?,
        )
    };
    let pointer_path = format!(
        "{}/{}",
        ws_path.trim_end_matches('/'),
        CATALOG_POINTERS_FILE
    );
    if !use_rest_catalog && op.exists(&pointer_path).await? {
        let bytes = op.read(&pointer_path).await?;
        let pointers: CatalogPointers = serde_json::from_slice(&bytes.to_vec())?;
        for pointer in pointers.tables {
            let namespace = NamespaceIdent::from_vec(pointer.namespace)?;
            if !catalog.namespace_exists(&namespace).await? {
                catalog.create_namespace(&namespace, HashMap::new()).await?;
            }
            let ident = TableIdent::new(namespace, pointer.table);
            if !catalog.table_exists(&ident).await? {
                catalog
                    .register_table(&ident, pointer.metadata_location)
                    .await?;
            }
        }
    }
    catalog_cache()
        .lock()
        .map_err(|_| anyhow!("catalog cache lock poisoned"))?
        .insert(cache_key, catalog.clone());
    Ok(catalog)
}

pub fn uses_rest_catalog() -> bool {
    std::env::var("UGOITE_ICEBERG_CATALOG_URI")
        .ok()
        .is_some_and(|uri| !uri.trim().is_empty())
}

async fn stable_space_id(op: &Operator, ws_path: &str) -> Result<ugoite_domain::id::SpaceId> {
    let meta_path = format!("{}/meta.json", ws_path.trim_end_matches('/'));
    if op.exists(&meta_path).await? {
        let metadata: Value = serde_json::from_slice(&op.read(&meta_path).await?.to_vec())?;
        if let Some(raw) = metadata
            .get("space_uid")
            .or_else(|| metadata.get("space_id"))
            .and_then(Value::as_str)
        {
            if let Ok(uuid) = Uuid::parse_str(raw) {
                return Ok(ugoite_domain::id::SpaceId::from(uuid));
            }
        }
    }
    Ok(ugoite_domain::id::SpaceId::from(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        ws_path.as_bytes(),
    )))
}

pub async fn native_workspace(
    op: &Operator,
    ws_path: &str,
) -> Result<ugoite_iceberg::IcebergWorkspace> {
    let catalog = catalog_for_space(op, ws_path).await?;
    let space_id = stable_space_id(op, ws_path).await?;
    ugoite_iceberg::IcebergWorkspace::new(
        catalog,
        space_id,
        warehouse_uri(op, ws_path)?,
        ugoite_iceberg::WriteConfig::default(),
    )
    .await
}

pub async fn persist_catalog_pointer(
    op: &Operator,
    ws_path: &str,
    table: &iceberg::table::Table,
) -> Result<()> {
    let pointer_path = format!(
        "{}/{}",
        ws_path.trim_end_matches('/'),
        CATALOG_POINTERS_FILE
    );
    let mut pointers = if op.exists(&pointer_path).await? {
        serde_json::from_slice::<CatalogPointers>(&op.read(&pointer_path).await?.to_vec())?
    } else {
        CatalogPointers {
            version: 1,
            tables: Vec::new(),
        }
    };
    let ident = table.identifier();
    let pointer = CatalogTablePointer {
        namespace: ident.namespace().as_ref().clone(),
        table: ident.name().to_string(),
        metadata_location: table.metadata_location_result()?.to_string(),
        form_id: table.metadata().properties().get("ugoite.form_id").cloned(),
        form_name: table
            .metadata()
            .properties()
            .get(FORM_DEF_PROP)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|definition| {
                definition
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }),
    };
    pointers.tables.retain(|existing| {
        existing.namespace != pointer.namespace || existing.table != pointer.table
    });
    pointers.tables.push(pointer);
    pointers.tables.sort_by(|left, right| {
        left.namespace
            .cmp(&right.namespace)
            .then_with(|| left.table.cmp(&right.table))
    });
    op.write(&pointer_path, serde_json::to_vec_pretty(&pointers)?)
        .await?;
    Ok(())
}

fn form_namespace(form_def: &Value) -> Result<NamespaceIdent> {
    form_def
        .get("id")
        .and_then(Value::as_str)
        .context("Form definition missing stable 'id'")?;
    // The warehouse is already scoped to one Space. Forms therefore share a
    // namespace and use their stable UUID as the physical table identity.
    Ok(NamespaceIdent::new("space".to_string()))
}

fn physical_form_table_name(form_def: &Value) -> Result<String> {
    let form_id = form_def
        .get("id")
        .and_then(Value::as_str)
        .context("Form definition missing stable 'id'")?;
    Ok(format!("form_{}", form_id.replace('-', "")))
}

async fn resolve_form_table_ident(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<TableIdent> {
    if uses_rest_catalog() {
        let catalog = catalog_for_space(op, ws_path).await?;
        let namespace = NamespaceIdent::new("space".to_string());
        for ident in catalog.list_tables(&namespace).await? {
            let table = catalog.load_table(&ident).await?;
            if let Some(raw) = table.metadata().properties().get(FORM_DEF_PROP) {
                let definition: Value = serde_json::from_str(raw)?;
                if definition.get("name").and_then(Value::as_str) == Some(form_name) {
                    return Ok(ident);
                }
            }
        }
        return Err(anyhow!("Form {form_name} not found in REST Catalog"));
    }
    let pointer_path = format!(
        "{}/{}",
        ws_path.trim_end_matches('/'),
        CATALOG_POINTERS_FILE
    );
    let bytes = op
        .read(&pointer_path)
        .await
        .with_context(|| format!("Catalog pointer manifest missing: {pointer_path}"))?;
    let pointers: CatalogPointers = serde_json::from_slice(&bytes.to_vec())?;
    let pointer = pointers
        .tables
        .into_iter()
        .find(|pointer| pointer.form_name.as_deref() == Some(form_name))
        .context("Form is not registered in the Catalog pointer manifest")?;
    Ok(TableIdent::new(
        NamespaceIdent::from_vec(pointer.namespace)?,
        pointer.table,
    ))
}

fn form_field_defs(form_def: &Value) -> Result<Vec<(i32, String, String, bool)>> {
    let mut fields = Vec::new();
    let Some(def_fields) = form_def.get("fields") else {
        return Ok(fields);
    };

    match def_fields {
        Value::Object(map) => {
            for (name, def) in map {
                let field_id = def
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .context("Form field missing stable id")?;
                let field_type = def
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string")
                    .to_string();
                let required = def
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                fields.push((field_id, name.clone(), field_type, required));
            }
        }
        Value::Array(items) => {
            for item in items {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let field_id = item
                    .get("id")
                    .and_then(Value::as_i64)
                    .and_then(|value| i32::try_from(value).ok())
                    .context("Form field missing stable id")?;
                let field_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string")
                    .to_string();
                let required = item
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                fields.push((field_id, name.to_string(), field_type, required));
            }
        }
        _ => {}
    }

    Ok(fields)
}

fn next_id(counter: &mut i32) -> i32 {
    let id = *counter;
    *counter += 1;
    id
}

fn iceberg_type_for_field(field_type: &str, id_counter: &mut i32) -> Result<Type> {
    Ok(match field_type {
        "number" | "double" => Type::Primitive(PrimitiveType::Double),
        "float" => Type::Primitive(PrimitiveType::Float),
        "integer" => Type::Primitive(PrimitiveType::Int),
        "long" => Type::Primitive(PrimitiveType::Long),
        "boolean" => Type::Primitive(PrimitiveType::Boolean),
        "date" => Type::Primitive(PrimitiveType::Date),
        "time" => Type::Primitive(PrimitiveType::Time),
        "timestamp" => Type::Primitive(PrimitiveType::Timestamp),
        "timestamp_tz" => Type::Primitive(PrimitiveType::Timestamptz),
        "timestamp_ns" => Type::Primitive(PrimitiveType::TimestampNs),
        "timestamp_tz_ns" => Type::Primitive(PrimitiveType::TimestamptzNs),
        "uuid" => Type::Primitive(PrimitiveType::Uuid),
        "binary" => Type::Primitive(PrimitiveType::Binary),
        "list" => {
            let element_id = next_id(id_counter);
            let element = Arc::new(NestedField::new(
                element_id,
                "element",
                Type::Primitive(PrimitiveType::String),
                false,
            ));
            Type::List(ListType::new(element))
        }
        "object_list" => {
            let element_id = next_id(id_counter);
            let struct_fields = vec![
                Arc::new(NestedField::new(
                    next_id(id_counter),
                    "type",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    next_id(id_counter),
                    "name",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    next_id(id_counter),
                    "description",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
            ];
            let struct_type = Type::Struct(StructType::new(struct_fields));
            let element = Arc::new(NestedField::new(element_id, "element", struct_type, false));
            Type::List(ListType::new(element))
        }
        "sql" | "markdown" | "string" | "row_reference" => Type::Primitive(PrimitiveType::String),
        _ => Type::Primitive(PrimitiveType::String),
    })
}

fn build_fields_struct(form_def: &Value, id_counter: &mut i32) -> Result<Type> {
    let mut nested_fields = Vec::new();
    for (field_id, name, field_type, required) in form_field_defs(form_def)? {
        let field_type = iceberg_type_for_field(&field_type, id_counter)?;
        nested_fields.push(Arc::new(NestedField::new(
            field_id, name, field_type, required,
        )));
    }

    Ok(Type::Struct(StructType::new(nested_fields)))
}

#[allow(dead_code)] // schema for frozen pre-refactor migration fixtures
fn build_entries_schema(form_def: &Value) -> Result<Schema> {
    let mut counter = 1;

    let tags_element_id = next_id(&mut counter);
    let tags_type = Type::List(ListType::new(Arc::new(NestedField::new(
        tags_element_id,
        "element",
        Type::Primitive(PrimitiveType::String),
        false,
    ))));

    let links_struct = Type::Struct(StructType::new(vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "target",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "kind",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ]));
    let links_element_id = next_id(&mut counter);
    let links_type = Type::List(ListType::new(Arc::new(NestedField::new(
        links_element_id,
        "element",
        links_struct,
        false,
    ))));

    let fields_struct = build_fields_struct(form_def, &mut counter)?;

    let assets_struct = Type::Struct(StructType::new(vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "name",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "path",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ]));
    let assets_element_id = next_id(&mut counter);
    let assets_type = Type::List(ListType::new(Arc::new(NestedField::new(
        assets_element_id,
        "element",
        assets_struct,
        false,
    ))));

    let integrity_struct = Type::Struct(StructType::new(vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "checksum",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "signature",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ]));

    let fields = vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "entry_id",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "title",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "tags",
            tags_type,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "links",
            links_type,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "created_at",
            Type::Primitive(PrimitiveType::Timestamp),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "updated_at",
            Type::Primitive(PrimitiveType::Timestamp),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "fields",
            fields_struct,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "extra_attributes",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "assets",
            assets_type,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "integrity",
            integrity_struct,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "deleted",
            Type::Primitive(PrimitiveType::Boolean),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "deleted_at",
            Type::Primitive(PrimitiveType::Timestamp),
            false,
        )),
    ];

    Schema::builder()
        .with_fields(fields)
        .build()
        .map_err(|e| e.into())
}

fn build_revisions_schema(form_def: &Value) -> Result<Schema> {
    let mut counter = 1;
    let fields_struct = build_fields_struct(form_def, &mut counter)?;
    let integrity_struct = Type::Struct(StructType::new(vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "checksum",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "signature",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ]));

    let fields = vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "revision_id",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "entry_id",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "parent_revision_id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "timestamp",
            Type::Primitive(PrimitiveType::Timestamp),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "author",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "fields",
            fields_struct,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "extra_attributes",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "markdown_checksum",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "integrity",
            integrity_struct,
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "restored_from",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "state_json",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "entry_version",
            Type::Primitive(PrimitiveType::Long),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "operation",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "source_kind",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "source_id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ];

    Schema::builder()
        .with_fields(fields)
        .build()
        .map_err(|e| e.into())
}

fn table_properties(form_def: &Value) -> Result<HashMap<String, String>> {
    let mut props = HashMap::new();
    let form_def_str = serde_json::to_string(form_def)?;
    props.insert(FORM_DEF_PROP.to_string(), form_def_str);
    let version = form_def
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    props.insert(FORM_VERSION_PROP.to_string(), version.to_string());
    Ok(props)
}

pub async fn ensure_form_tables(op: &Operator, ws_path: &str, form_def: &Value) -> Result<()> {
    let use_rest_catalog = uses_rest_catalog();
    let catalog = catalog_for_space(op, ws_path).await?;
    let namespace = form_namespace(form_def)?;

    if !catalog.namespace_exists(&namespace).await? {
        if let Err(err) = catalog.create_namespace(&namespace, HashMap::new()).await {
            let message = err.to_string();
            if !message.contains("NamespaceAlreadyExists")
                && !message.to_lowercase().contains("already exists")
            {
                return Err(err.into());
            }
        }
    }

    let revisions_ident = TableIdent::new(namespace.clone(), physical_form_table_name(form_def)?);
    if !catalog.table_exists(&revisions_ident).await? {
        let schema = build_revisions_schema(form_def)?;
        let props = table_properties(form_def)?;
        let creation = TableCreation::builder()
            .name(physical_form_table_name(form_def)?)
            .schema(schema)
            .partition_spec(UnboundPartitionSpec::default())
            .sort_order(SortOrder::unsorted_order())
            .properties(props)
            .build();
        let created = catalog.create_table(&namespace, creation).await;
        if let Err(err) = created {
            let message = err.to_string();
            if !message.contains("TableAlreadyExists") && !message.contains("already exists") {
                return Err(err.into());
            }
            let props = table_properties(form_def)?;
            let table = catalog.load_table(&revisions_ident).await?;
            let tx = Transaction::new(&table);
            let mut action = tx.update_table_properties();
            for (key, value) in props {
                action = action.set(key, value);
            }
            let tx = action.apply(tx)?;
            tx.commit(catalog.as_ref()).await?;
        }
    } else {
        let props = table_properties(form_def)?;
        let table = catalog.load_table(&revisions_ident).await?;
        let tx = Transaction::new(&table);
        let mut action = tx.update_table_properties();
        for (key, value) in props {
            action = action.set(key, value);
        }
        let tx = action.apply(tx)?;
        tx.commit(catalog.as_ref()).await?;
    }

    let table = catalog.load_table(&revisions_ident).await?;
    if !use_rest_catalog {
        persist_catalog_pointer(op, ws_path, &table).await?;
    }

    Ok(())
}

pub async fn load_entries_table(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<(Arc<dyn Catalog>, iceberg::table::Table)> {
    let catalog = catalog_for_space(op, ws_path).await?;
    let table_ident = resolve_form_table_ident(op, ws_path, form_name).await?;
    let table = catalog.load_table(&table_ident).await?;
    Ok((catalog, table))
}

pub async fn load_revisions_table(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<(Arc<dyn Catalog>, iceberg::table::Table)> {
    let catalog = catalog_for_space(op, ws_path).await?;
    let revisions_ident = resolve_form_table_ident(op, ws_path, form_name).await?;
    let revisions = catalog.load_table(&revisions_ident).await?;
    Ok((catalog, revisions))
}

pub async fn load_form_schema_fields(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<Option<std::collections::HashSet<String>>> {
    let (_, table) = load_revisions_table(op, ws_path, form_name).await?;
    let Some(field) = table.metadata().current_schema().field_by_name("fields") else {
        return Ok(None);
    };
    let Type::Struct(fields) = field.field_type.as_ref() else {
        return Ok(None);
    };
    Ok(Some(
        fields
            .fields()
            .iter()
            .map(|field| field.name.clone())
            .collect(),
    ))
}

pub async fn list_form_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    if uses_rest_catalog() {
        let catalog = catalog_for_space(op, ws_path).await?;
        let namespace = NamespaceIdent::new("space".to_string());
        let mut names = Vec::new();
        for ident in catalog.list_tables(&namespace).await? {
            let table = catalog.load_table(&ident).await?;
            if let Some(raw) = table.metadata().properties().get(FORM_DEF_PROP) {
                let definition: Value = serde_json::from_str(raw)?;
                if let Some(name) = definition.get("name").and_then(Value::as_str) {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        return Ok(names);
    }
    let pointer_path = format!(
        "{}/{}",
        ws_path.trim_end_matches('/'),
        CATALOG_POINTERS_FILE
    );
    if !op.exists(&pointer_path).await? {
        return Ok(Vec::new());
    }
    let bytes = op.read(&pointer_path).await?;
    let pointers: CatalogPointers = serde_json::from_slice(&bytes.to_vec())?;
    let mut names: Vec<String> = pointers
        .tables
        .into_iter()
        .filter_map(|pointer| pointer.form_name)
        .collect();
    names.sort();
    Ok(names)
}

pub async fn load_form_definition(op: &Operator, ws_path: &str, form_name: &str) -> Result<Value> {
    let (_, entries) = load_entries_table(op, ws_path, form_name).await?;
    let props = entries.metadata().properties();
    let Some(definition) = props.get(FORM_DEF_PROP) else {
        return Err(anyhow!("Form definition missing in Iceberg metadata"));
    };
    let form_def = serde_json::from_str::<Value>(definition)?;
    Ok(form_def)
}
