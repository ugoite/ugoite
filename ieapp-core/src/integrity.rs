pub trait IntegrityProvider {
    fn checksum(&self, content: &str) -> String;
    fn signature(&self, content: &str) -> String;
}

pub struct FakeIntegrityProvider;

impl IntegrityProvider for FakeIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        format!("mock-checksum-{}", content.len())
    }
    fn signature(&self, content: &str) -> String {
        format!("mock-signature-{}", content.len())
    }
}
