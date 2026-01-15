use anyhow::Result;
use opendal::Operator;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static MEMORY_OPERATORS: OnceLock<Mutex<HashMap<String, Operator>>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<HashMap<String, Operator>> {
    MEMORY_OPERATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn operator_from_uri(uri: &str) -> Result<Operator> {
    if uri.starts_with("memory://") {
        let mut cache = memory_cache()
            .lock()
            .map_err(|_| anyhow::anyhow!("memory operator cache lock poisoned"))?;
        if let Some(op) = cache.get(uri) {
            return Ok(op.clone());
        }
        let op = Operator::from_uri(uri)?;
        cache.insert(uri.to_string(), op.clone());
        return Ok(op);
    }

    Ok(Operator::from_uri(uri)?)
}
