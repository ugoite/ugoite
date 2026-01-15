use anyhow::Result;
use opendal::services::Memory;
use opendal::Operator;

#[allow(dead_code)]
pub fn setup_operator() -> Result<Operator> {
    let builder = Memory::default();
    let op = Operator::new(builder)?.finish();
    Ok(op)
}
