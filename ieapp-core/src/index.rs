use anyhow::Result;
use opendal::Operator;
use regex::Regex;

pub async fn query_index(_op: &Operator, _ws_path: &str, query: &str) -> Result<String> {
    Ok(format!("{{ \"results\": [], \"query\": \"{}\" }}", query))
}

pub async fn reindex_all(op: &Operator, ws_path: &str) -> Result<()> {
    // Basic reindex logic: ensuring index files exist and are valid JSON
    // Real implementation would scan notes/ files and rebuild index.json

    let index_path = format!("{}/index/index.json", ws_path);
    let stats_path = format!("{}/index/stats.json", ws_path);

    // Reset index files
    op.write(&index_path, b"{}" as &[u8]).await?;
    op.write(&stats_path, b"{}" as &[u8]).await?;

    Ok(())
}

pub fn extract_properties(markdown: &str) -> serde_json::Value {
    let mut properties = serde_json::Map::new();

    // 1. Extract frontmatter
    let (frontmatter, body) = extract_frontmatter(markdown);
    if let Some(fm) = frontmatter {
        if let Some(obj) = fm.as_object() {
            for (k, v) in obj {
                properties.insert(k.clone(), v.clone());
            }
        }
    }

    // 2. Extract sections (H2)
    let sections = extract_sections(&body);
    for (k, v) in sections {
        // Only insert if not empty string? Python: `if value:` (implied check)
        if !v.is_empty() {
            properties.insert(k, serde_json::Value::String(v));
        }
    }

    serde_json::Value::Object(properties)
}

fn extract_frontmatter(content: &str) -> (Option<serde_json::Value>, String) {
    // (?s) enables dot-matches-newline
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(caps) = re.captures(content) {
        let yaml_str = caps.get(1).unwrap().as_str();
        // Convert yaml::Value to json::Value via serde
        let fm_yaml: Option<serde_yaml::Value> = serde_yaml::from_str(yaml_str).ok();

        let fm_json = if let Some(y) = fm_yaml {
            serde_json::to_value(y).ok()
        } else {
            None
        };

        let end = caps.get(0).unwrap().end();
        return (fm_json, content[end..].to_string());
    }
    (None, content.to_string())
}

fn extract_sections(body: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    let header_re = Regex::new(r"^##\s+(.+)$").unwrap();

    for line in body.lines() {
        if let Some(caps) = header_re.captures(line) {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            current_key = Some(caps.get(1).unwrap().as_str().trim().to_string());
            buffer.clear();
            continue;
        }

        // Other headers stop current section
        if line.starts_with("#") {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            buffer.clear();
            continue;
        }

        if current_key.is_some() {
            buffer.push(line.to_string());
        }
    }

    if let Some(key) = current_key {
        sections.push((key, buffer.join("\n").trim().to_string()));
    }

    sections
}

pub fn compute_word_count(content: &str) -> usize {
    content.split_whitespace().count()
}

pub async fn validate_properties(
    _op: &Operator,
    _ws_path: &str,
    _class_name: &str,
    _properties: &serde_json::Value,
) -> Result<Vec<String>> {
    // Stub implementation
    Ok(Vec::new())
}
