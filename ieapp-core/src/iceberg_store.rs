use anyhow::{anyhow, Result};
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use iceberg::spec::{ListType, NestedField, Schema, StructType, Type, UnboundPartitionSpec};
use iceberg::spec::{PrimitiveType, SortOrder};
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::{Catalog, CatalogBuilder, MemoryCatalog, NamespaceIdent, TableCreation, TableIdent};
use opendal::Operator;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

const NOTES_TABLE_NAME: &str = "notes";
const REVISIONS_TABLE_NAME: &str = "revisions";
const CLASS_DEF_PROP: &str = "ieapp.class_definition";
const CLASS_VERSION_PROP: &str = "ieapp.class_version";

static CATALOG_CACHE: OnceLock<Mutex<HashMap<String, Arc<MemoryCatalog>>>> = OnceLock::new();

fn catalog_cache() -> &'static Mutex<HashMap<String, Arc<MemoryCatalog>>> {
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
    Ok(format!("{}{}{}", prefix, warehouse_path, "/classes"))
}

async fn catalog_for_workspace(op: &Operator, ws_path: &str) -> Result<Arc<MemoryCatalog>> {
    let warehouse = warehouse_uri(op, ws_path)?;
    {
        let cache = catalog_cache()
            .lock()
            .map_err(|_| anyhow!("catalog cache lock poisoned"))?;
        if let Some(catalog) = cache.get(&warehouse) {
            return Ok(Arc::clone(catalog));
        }
    }

    let mut props = HashMap::new();
    props.insert(MEMORY_CATALOG_WAREHOUSE.to_string(), warehouse.clone());
    let catalog: MemoryCatalog = MemoryCatalogBuilder::default().load("ieapp", props).await?;
    let catalog = Arc::new(catalog);
    let mut cache = catalog_cache()
        .lock()
        .map_err(|_| anyhow!("catalog cache lock poisoned"))?;
    cache.entry(warehouse).or_insert_with(|| catalog.clone());
    Ok(catalog)
}

fn class_namespace(class_name: &str) -> NamespaceIdent {
    NamespaceIdent::new(class_name.to_string())
}

fn class_field_defs(class_def: &Value) -> Result<Vec<(String, String, bool)>> {
    let mut fields = Vec::new();
    let Some(def_fields) = class_def.get("fields") else {
        return Ok(fields);
    };

    match def_fields {
        Value::Object(map) => {
            for (name, def) in map {
                let field_type = def
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string")
                    .to_string();
                let required = def
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                fields.push((name.clone(), field_type, required));
            }
        }
        Value::Array(items) => {
            for item in items {
                let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let field_type = item
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("string")
                    .to_string();
                let required = item
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                fields.push((name.to_string(), field_type, required));
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
        "number" => Type::Primitive(PrimitiveType::Double),
        "date" => Type::Primitive(PrimitiveType::Date),
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
        _ => Type::Primitive(PrimitiveType::String),
    })
}

fn build_fields_struct(class_def: &Value, id_counter: &mut i32) -> Result<Type> {
    let mut nested_fields = Vec::new();
    for (name, field_type, required) in class_field_defs(class_def)? {
        let field_id = next_id(id_counter);
        let field_type = iceberg_type_for_field(&field_type, id_counter)?;
        nested_fields.push(Arc::new(NestedField::new(
            field_id, name, field_type, required,
        )));
    }

    Ok(Type::Struct(StructType::new(nested_fields)))
}

fn build_notes_schema(_class_def: &Value) -> Result<Schema> {
    let mut counter = 1;
    let fields = vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "note_id",
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
            "class",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "tags",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "links",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "canvas_position",
            Type::Primitive(PrimitiveType::String),
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
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "revision_id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "parent_revision_id",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "attachments",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "integrity",
            Type::Primitive(PrimitiveType::String),
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
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "author",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ];

    Schema::builder()
        .with_fields(fields)
        .build()
        .map_err(|e| e.into())
}

fn build_revisions_schema(_class_def: &Value) -> Result<Schema> {
    let mut counter = 1;
    let fields = vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "revision_id",
            Type::Primitive(PrimitiveType::String),
            true,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "note_id",
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
            Type::Primitive(PrimitiveType::String),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "restored_from",
            Type::Primitive(PrimitiveType::String),
            false,
        )),
    ];

    Schema::builder()
        .with_fields(fields)
        .build()
        .map_err(|e| e.into())
}

fn table_properties(class_def: &Value) -> Result<HashMap<String, String>> {
    let mut props = HashMap::new();
    let class_def_str = serde_json::to_string(class_def)?;
    props.insert(CLASS_DEF_PROP.to_string(), class_def_str);
    let version = class_def
        .get("version")
        .and_then(|v| v.as_i64())
        .unwrap_or(1);
    props.insert(CLASS_VERSION_PROP.to_string(), version.to_string());
    Ok(props)
}

pub async fn ensure_class_tables(op: &Operator, ws_path: &str, class_def: &Value) -> Result<()> {
    let class_name = class_def
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Class definition missing 'name'"))?;
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespace = class_namespace(class_name);

    if !catalog.namespace_exists(&namespace).await? {
        catalog.create_namespace(&namespace, HashMap::new()).await?;
    }

    let notes_ident = TableIdent::new(namespace.clone(), NOTES_TABLE_NAME.to_string());
    if !catalog.table_exists(&notes_ident).await? {
        let schema = build_notes_schema(class_def)?;
        let props = table_properties(class_def)?;
        let creation = TableCreation::builder()
            .name(NOTES_TABLE_NAME.to_string())
            .schema(schema)
            .partition_spec(UnboundPartitionSpec::default())
            .sort_order(SortOrder::unsorted_order())
            .properties(props)
            .build();
        catalog.create_table(&namespace, creation).await?;
    } else {
        let props = table_properties(class_def)?;
        let table = catalog.load_table(&notes_ident).await?;
        let tx = Transaction::new(&table);
        let mut action = tx.update_table_properties();
        for (key, value) in props {
            action = action.set(key, value);
        }
        let tx = action.apply(tx)?;
        tx.commit(catalog.as_ref()).await?;
    }

    let revisions_ident = TableIdent::new(namespace.clone(), REVISIONS_TABLE_NAME.to_string());
    if !catalog.table_exists(&revisions_ident).await? {
        let schema = build_revisions_schema(class_def)?;
        let props = table_properties(class_def)?;
        let creation = TableCreation::builder()
            .name(REVISIONS_TABLE_NAME.to_string())
            .schema(schema)
            .partition_spec(UnboundPartitionSpec::default())
            .sort_order(SortOrder::unsorted_order())
            .properties(props)
            .build();
        catalog.create_table(&namespace, creation).await?;
    } else {
        let props = table_properties(class_def)?;
        let table = catalog.load_table(&revisions_ident).await?;
        let tx = Transaction::new(&table);
        let mut action = tx.update_table_properties();
        for (key, value) in props {
            action = action.set(key, value);
        }
        let tx = action.apply(tx)?;
        tx.commit(catalog.as_ref()).await?;
    }

    Ok(())
}

pub async fn load_class_tables(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<(
    Arc<MemoryCatalog>,
    iceberg::table::Table,
    iceberg::table::Table,
)> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespace = class_namespace(class_name);
    let notes_ident = TableIdent::new(namespace.clone(), NOTES_TABLE_NAME.to_string());
    let revisions_ident = TableIdent::new(namespace.clone(), REVISIONS_TABLE_NAME.to_string());

    let notes = catalog.load_table(&notes_ident).await?;
    let revisions = catalog.load_table(&revisions_ident).await?;
    Ok((catalog, notes, revisions))
}

pub async fn load_notes_table(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<(Arc<MemoryCatalog>, iceberg::table::Table)> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespace = class_namespace(class_name);
    let notes_ident = TableIdent::new(namespace, NOTES_TABLE_NAME.to_string());
    let notes = catalog.load_table(&notes_ident).await?;
    Ok((catalog, notes))
}

pub async fn load_revisions_table(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<(Arc<MemoryCatalog>, iceberg::table::Table)> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespace = class_namespace(class_name);
    let revisions_ident = TableIdent::new(namespace, REVISIONS_TABLE_NAME.to_string());
    let revisions = catalog.load_table(&revisions_ident).await?;
    Ok((catalog, revisions))
}

pub async fn list_class_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespaces: Vec<NamespaceIdent> = catalog.list_namespaces(None).await?;
    Ok(namespaces
        .into_iter()
        .map(|ns: NamespaceIdent| ns.to_string())
        .collect())
}

pub async fn load_class_definition(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<Value> {
    let (_, notes): (Arc<MemoryCatalog>, iceberg::table::Table) =
        load_notes_table(op, ws_path, class_name).await?;
    let props = notes.metadata().properties();
    let Some(definition) = props.get(CLASS_DEF_PROP) else {
        return Err(anyhow!("Class definition missing in Iceberg metadata"));
    };
    let class_def = serde_json::from_str::<Value>(definition)?;
    Ok(class_def)
}
