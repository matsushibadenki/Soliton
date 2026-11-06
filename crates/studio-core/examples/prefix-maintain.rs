//! Offline capacity inspection/trim. Does not replace the qualified inference store executable.
use std::path::Path;
use studio_core::prefix_store::Store;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() != 4 {
        return Err("Usage: prefix-maintain STORE_ROOT QUOTA_BYTES stats|trim".into());
    }
    let store = Store::existing(Path::new(&args[1]), args[2].parse()?)?;
    let result = match args[3].as_str() {
        "stats" => serde_json::to_value(store.capacity()?)?,
        "trim" => serde_json::to_value(store.trim()?)?,
        _ => return Err("Unknown maintenance operation".into()),
    };
    println!("{result}");
    Ok(())
}
