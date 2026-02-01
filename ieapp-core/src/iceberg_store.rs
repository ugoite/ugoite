use anyhow::{anyhow, Result};
use futures::TryStreamExt;
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use iceberg::spec::{ListType, NestedField, Schema, StructType, Type, UnboundPartitionSpec};
use iceberg::spec::{PrimitiveType, SortOrder};
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::{Catalog, CatalogBuilder, MemoryCatalog, NamespaceIdent, TableCreation, TableIdent};
use opendal::{options, Operator};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

const NOTES_TABLE_NAME: &str = "notes";
const REVISIONS_TABLE_NAME: &str = "revisions";
const CLASS_DEF_PROP: &str = "ieapp.class_definition";
const CLASS_VERSION_PROP: &str = "ieapp.class_version";

static CATALOG_CACHE: OnceLock<Mutex<HashMap<String, Arc<MemoryCatalog>>>> = OnceLock::new();
fn catalog_cache() -> &'static Mutex<HashMap<String, Arc<MemoryCatalog>>> {
    CATALOG_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remove_catalog_cache(warehouse: &str) -> Result<()> {
    let mut cache = catalog_cache()
        .lock()
        .map_err(|_| anyhow!("catalog cache lock poisoned"))?;
    cache.remove(warehouse);
    Ok(())
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

fn table_location(warehouse: &str, class_name: &str, table_name: &str) -> String {
    format!(
        "{}/{}/{}",
        warehouse.trim_end_matches('/'),
        class_name,
        table_name
    )
}

fn metadata_location(
    warehouse: &str,
    class_name: &str,
    table_name: &str,
    file_name: &str,
) -> String {
    format!(
        "{}/metadata/{}",
        table_location(warehouse, class_name, table_name),
        file_name
    )
}

fn parse_metadata_version(file_name: &str) -> Option<i32> {
    let file_name = file_name.split('/').next_back()?;
    let base = file_name.strip_suffix(".metadata.json")?;
    let (version, _) = base.split_once('-')?;
    version.parse::<i32>().ok()
}

async fn latest_metadata_file(op: &Operator, metadata_path: &str) -> Result<Option<String>> {
    let scheme = op.info().scheme();
    if scheme == "fs" || scheme == "file" {
        let root = normalize_root(op.info().root().as_str());
        let fs_path = format!("{}/{}", root, metadata_path.trim_start_matches('/'));
        if let Ok(entries) = std::fs::read_dir(&fs_path) {
            let mut latest: Option<(i32, String)> = None;
            for entry in entries.flatten() {
                let file_name = match entry.file_name().to_str() {
                    Some(name) => name.to_string(),
                    None => continue,
                };
                let Some(version) = parse_metadata_version(&file_name) else {
                    continue;
                };
                let replace = match latest {
                    Some((current, _)) => version > current,
                    None => true,
                };
                if replace {
                    latest = Some((version, file_name));
                }
            }
            return Ok(latest.map(|(_, name)| name));
        }
    }

    let mut lister = match op
        .lister_options(
            metadata_path,
            options::ListOptions {
                recursive: true,
                ..Default::default()
            },
        )
        .await
    {
        Ok(lister) => lister,
        Err(_) => return Ok(None),
    };
    let mut latest: Option<(i32, String)> = None;

    while let Some(entry) = lister.try_next().await? {
        let name = entry.path();
        let Some(version) = parse_metadata_version(name) else {
            continue;
        };
        let file_name = name.split('/').next_back().unwrap_or("").to_string();
        if file_name.is_empty() {
            continue;
        }
        let replace = match latest {
            Some((current, _)) => version > current,
            None => true,
        };
        if replace {
            latest = Some((version, file_name));
        }
    }

    Ok(latest.map(|(_, name)| name))
}

async fn list_class_dirs(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    let classes_path = format!("{}/classes/", ws_path.trim_end_matches('/'));
    let mut lister = match op
        .lister_options(
            &classes_path,
            options::ListOptions {
                recursive: false,
                ..Default::default()
            },
        )
        .await
    {
        Ok(lister) => lister,
        Err(_) => return Ok(Vec::new()),
    };

    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let classes_prefix = classes_path.trim_end_matches('/');
    while let Some(entry) = lister.try_next().await? {
        if !entry.metadata().is_dir() {
            continue;
        }
        let path = entry.path().trim_end_matches('/');
        let relative = match path.strip_prefix(classes_prefix) {
            Some(rest) => rest.trim_start_matches('/'),
            None => continue,
        };
        if relative.is_empty() || relative.contains('/') {
            continue;
        }
        if seen.contains(relative) {
            continue;
        }
        seen.insert(relative.to_string());
        names.push(relative.to_string());
    }

    Ok(names)
}

async fn register_existing_tables(
    op: &Operator,
    ws_path: &str,
    catalog: &MemoryCatalog,
) -> Result<()> {
    let class_names = list_class_dirs(op, ws_path).await?;
    if class_names.is_empty() {
        return Ok(());
    }

    let warehouse = warehouse_uri(op, ws_path)?;
    for class_name in class_names {
        let namespace = class_namespace(&class_name);
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

        for table_name in [NOTES_TABLE_NAME, REVISIONS_TABLE_NAME] {
            let table_ident = TableIdent::new(namespace.clone(), table_name.to_string());
            if catalog.table_exists(&table_ident).await? {
                continue;
            }

            let metadata_path = format!(
                "{}/classes/{}/{}/metadata/",
                ws_path.trim_end_matches('/'),
                class_name,
                table_name
            );
            let Some(latest) = latest_metadata_file(op, &metadata_path).await? else {
                continue;
            };
            let metadata_location = metadata_location(&warehouse, &class_name, table_name, &latest);
            catalog
                .register_table(&table_ident, metadata_location)
                .await?;
        }
    }

    Ok(())
}

async fn catalog_for_workspace(op: &Operator, ws_path: &str) -> Result<Arc<MemoryCatalog>> {
    let warehouse = warehouse_uri(op, ws_path)?;
    if let Some(cached) = {
        let cache = catalog_cache()
            .lock()
            .map_err(|_| anyhow!("catalog cache lock poisoned"))?;
        cache.get(&warehouse).cloned()
    } {
        register_existing_tables(op, ws_path, cached.as_ref()).await?;
        return Ok(cached);
    }

    let mut props = HashMap::new();
    props.insert(MEMORY_CATALOG_WAREHOUSE.to_string(), warehouse.clone());
    let catalog: MemoryCatalog = MemoryCatalogBuilder::default().load("ieapp", props).await?;
    register_existing_tables(op, ws_path, &catalog).await?;
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
        "markdown" | "string" => Type::Primitive(PrimitiveType::String),
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

fn build_notes_schema(class_def: &Value) -> Result<Schema> {
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

    let canvas_struct = Type::Struct(StructType::new(vec![
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "x",
            Type::Primitive(PrimitiveType::Double),
            false,
        )),
        Arc::new(NestedField::new(
            next_id(&mut counter),
            "y",
            Type::Primitive(PrimitiveType::Double),
            false,
        )),
    ]));

    let fields_struct = build_fields_struct(class_def, &mut counter)?;

    let attachments_struct = Type::Struct(StructType::new(vec![
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
    let attachments_element_id = next_id(&mut counter);
    let attachments_type = Type::List(ListType::new(Arc::new(NestedField::new(
        attachments_element_id,
        "element",
        attachments_struct,
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
            "canvas_position",
            canvas_struct,
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
            "attachments",
            attachments_type,
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

fn build_revisions_schema(class_def: &Value) -> Result<Schema> {
    let mut counter = 1;
    let fields_struct = build_fields_struct(class_def, &mut counter)?;
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
        if let Err(err) = catalog.create_namespace(&namespace, HashMap::new()).await {
            let message = err.to_string();
            if !message.contains("NamespaceAlreadyExists")
                && !message.to_lowercase().contains("already exists")
            {
                return Err(err.into());
            }
        }
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
        let created = catalog.create_table(&namespace, creation).await;
        if let Err(err) = created {
            let message = err.to_string();
            if !message.contains("TableAlreadyExists") && !message.contains("already exists") {
                return Err(err.into());
            }
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
        let created = catalog.create_table(&namespace, creation).await;
        if let Err(err) = created {
            let message = err.to_string();
            if !message.contains("TableAlreadyExists") && !message.contains("already exists") {
                return Err(err.into());
            }
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

pub async fn load_class_schema_fields(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<Option<std::collections::HashSet<String>>> {
    let metadata_dir = format!(
        "{}/classes/{}/notes/metadata/",
        ws_path.trim_end_matches('/'),
        class_name
    );
    let Some(latest) = latest_metadata_file(op, &metadata_dir).await? else {
        return Ok(None);
    };
    let metadata_path = format!("{}{}", metadata_dir, latest);
    let bytes = op.read(&metadata_path).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;
    let schemas = value.get("schemas").and_then(|v| v.as_array());
    let current_schema_id = value.get("current-schema-id").and_then(|v| v.as_i64());
    let schema = schemas.and_then(|arr| {
        if let Some(current_id) = current_schema_id {
            arr.iter()
                .find(|schema| schema.get("schema-id").and_then(|v| v.as_i64()) == Some(current_id))
                .or_else(|| arr.first())
        } else {
            arr.first()
        }
    });
    let Some(schema) = schema else {
        return Ok(None);
    };
    let fields = schema.get("fields").and_then(|v| v.as_array());
    let Some(fields) = fields else {
        return Ok(None);
    };
    for field in fields {
        if field.get("name").and_then(|v| v.as_str()) == Some("fields") {
            let struct_fields = field
                .get("type")
                .and_then(|v| v.get("fields"))
                .and_then(|v| v.as_array());
            let Some(struct_fields) = struct_fields else {
                return Ok(None);
            };
            let names = struct_fields
                .iter()
                .filter_map(|f| {
                    f.get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            return Ok(Some(names));
        }
    }
    Ok(None)
}

pub async fn drop_class_tables(op: &Operator, ws_path: &str, class_name: &str) -> Result<()> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespace = class_namespace(class_name);
    let notes_ident = TableIdent::new(namespace.clone(), NOTES_TABLE_NAME.to_string());
    let revisions_ident = TableIdent::new(namespace, REVISIONS_TABLE_NAME.to_string());

    if catalog.table_exists(&notes_ident).await? {
        catalog.drop_table(&notes_ident).await?;
    }
    if catalog.table_exists(&revisions_ident).await? {
        catalog.drop_table(&revisions_ident).await?;
    }

    let namespace = class_namespace(class_name);
    if catalog.namespace_exists(&namespace).await? {
        let _ = catalog.drop_namespace(&namespace).await;
    }

    let class_root = format!("{}/classes/{}/", ws_path.trim_end_matches('/'), class_name);
    let scheme = op.info().scheme();
    if scheme == "fs" || scheme == "file" {
        let root = normalize_root(op.info().root().as_str());
        let ws_path = ws_path.trim_start_matches('/');
        let fs_root = format!("{}/{}/classes/{}", root, ws_path, class_name);
        let _ = std::fs::remove_dir_all(&fs_root);
    } else {
        let _ = op.remove_all(&class_root).await;
    }

    let warehouse = warehouse_uri(op, ws_path)?;
    remove_catalog_cache(&warehouse)?;

    Ok(())
}

pub async fn list_class_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    let catalog: Arc<MemoryCatalog> = catalog_for_workspace(op, ws_path).await?;
    let namespaces = catalog.list_namespaces(None).await?;
    let mut names = Vec::new();
    for namespace in namespaces {
        if let Some(first) = namespace.as_ref().first() {
            names.push(first.clone());
        }
    }
    if names.is_empty() {
        return list_class_dirs(op, ws_path).await;
    }
    Ok(names)
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

pub async fn load_class_definition_from_metadata(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
) -> Result<Option<Value>> {
    let metadata_dir = format!(
        "{}/classes/{}/notes/metadata/",
        ws_path.trim_end_matches('/'),
        class_name
    );
    let Some(latest) = latest_metadata_file(op, &metadata_dir).await? else {
        return Ok(None);
    };
    let metadata_path = format!("{}{}", metadata_dir, latest);
    let bytes = op.read(&metadata_path).await?;
    let value: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;
    let props = value.get("properties").and_then(|v| v.as_object());
    let Some(props) = props else {
        return Ok(None);
    };
    let Some(definition) = props.get(CLASS_DEF_PROP).and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let class_def = serde_json::from_str::<Value>(definition)?;
    Ok(Some(class_def))
}
