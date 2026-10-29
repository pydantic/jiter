use std::process::Command;
use std::sync::Once;

static PREPARED: Once = Once::new();

pub fn prepare() {
    PREPARED.call_once(|| {
        let status = Command::new("python3")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/benches/prepare_documents.py"))
            .status()
            .expect("python3 is required to prepare document benchmarks");
        assert!(status.success(), "failed to prepare document benchmarks");
    });
}
