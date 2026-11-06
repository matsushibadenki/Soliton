//! Opt-in eviction candidate; requires separate qualification before managed-engine adoption.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
use studio_core::prefix_store::{Identity, Store};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "Usage: prefix-store-evict ROOT QUOTA_BYTES put|get IDENTITY_JSON PAYLOAD_PATH".into(),
        );
    }
    let store = Store::open(Path::new(&args[1]), args[2].parse()?)?;
    let identity: Identity = serde_json::from_slice(&std::fs::read(&args[4])?)?;
    match args[3].as_str() {
        "put" => {
            let mut file = File::open(&args[5])?;
            let n = file.metadata()?.len();
            let eviction = store.put_evicting(&identity, &mut file, n)?;
            println!(
                "{}",
                serde_json::json!({"entry":format!("{}.cache",identity.key()?),"bytes":n,"eviction":eviction})
            );
        }
        "get" => {
            let (file, n) = store.get(&identity)?.ok_or("Cache miss")?;
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(&args[5])?;
            io::copy(&mut file.take(n), &mut output)?;
            output.flush()?;
            output.sync_all()?;
            println!(
                "{}",
                serde_json::json!({"entry":format!("{}.cache",identity.key()?),"bytes":n})
            );
        }
        _ => return Err("Unknown cache operation".into()),
    }
    Ok(())
}
