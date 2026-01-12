use anyhow::{anyhow, Result};
use opendal::{Operator, Scheme};
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;

/// Creates an OpenDAL Operator from a URI string.
///
/// Supported schemes:
/// - file:///path/to/dir -> local filesystem
/// - s3://bucket/path -> AWS S3 (requires env vars like AWS_ACCESS_KEY_ID)
/// - memory:// -> In-memory (for testing)
pub fn create_operator_from_uri(uri: &str) -> Result<Operator> {
    if uri == "memory://" {
        let builder = opendal::services::Memory::default();
        let op = Operator::new(builder)?.finish();
        return Ok(op);
    }

    // Parse URI
    let url = Url::parse(uri).map_err(|e| anyhow!("Invalid storage URI: {}", e))?;
    let mut scheme_str = url.scheme();
    // Map "file" scheme to "fs" as expected by OpenDAL
    if scheme_str == "file" {
        scheme_str = "fs";
    }

    let scheme = Scheme::from_str(scheme_str)
        .map_err(|_| anyhow!("Unsupported storage scheme: {}", scheme_str))?;

    let mut map = HashMap::new();

    match scheme {
        Scheme::Fs => {
            // For file://, path is the root
            // url.path() returns the path part
            let root = url.path();
            map.insert("root".to_string(), root.to_string());
        }
        Scheme::S3 => {
            // s3://bucket/root
            let bucket = url
                .host_str()
                .ok_or_else(|| anyhow!("S3 URI missing bucket"))?;
            map.insert("bucket".to_string(), bucket.to_string());
            let root = url.path();
            if !root.is_empty() && root != "/" {
                map.insert("root".to_string(), root.to_string());
            }
            // Region etc should be picked up from env or handled differently if needed
            map.insert("region".to_string(), "auto".to_string());
        }
        _ => {
            // For other schemes, try passing basic things
            return Err(anyhow!("Scheme {} implementation pending", scheme));
        }
    }

    let op = Operator::via_iter(scheme, map)?;
    Ok(op)
}
