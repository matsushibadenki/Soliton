//! Experimental local-only CLI; state payloads never cross the WebView boundary.
use std::{fs::File, path::Path, time::Instant};
use studio_core::prefix_store::{Identity, PutTimings, Store};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entered = Instant::now();
    let profile = std::env::var_os("SOLITON_PREFIX_STORE_TIMINGS").is_some();
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err(
            "Usage: prefix-store ROOT QUOTA_BYTES put|get IDENTITY_JSON PAYLOAD_PATH".into(),
        );
    }
    let store = Store::open(Path::new(&args[1]), args[2].parse()?)?;
    let identity: Identity = serde_json::from_slice(&std::fs::read(&args[4])?)?;
    let setup_ns = entered.elapsed().as_nanos();
    let phases = match args[3].as_str() {
        "put" => {
            let mut file = File::open(&args[5])?;
            let n = file.metadata()?.len();
            let mut timing = PutTimings::default();
            if profile {
                store.put_timed(&identity, &mut file, n, &mut timing)?;
            } else {
                store.put(&identity, &mut file, n)?;
            }
            println!(
                "{}",
                serde_json::json!({"entry":format!("{}.cache",identity.key()?),"bytes":n})
            );
            serde_json::to_value(timing)?
        }
        "get" => {
            let started = Instant::now();
            let (n, durability_ns) = store
                .restore_to(&identity, Path::new(&args[5]))?
                .ok_or("Cache miss")?;
            let restore_ns = started.elapsed().as_nanos();
            println!(
                "{}",
                serde_json::json!({"entry":format!("{}.cache",identity.key()?),"bytes":n})
            );
            serde_json::json!({
                "verifyAndCopyNs": restore_ns - durability_ns,
                "durabilityNs": durability_ns,
            })
        }
        _ => return Err("Unknown cache operation".into()),
    };
    if profile {
        eprintln!(
            "{}",
            serde_json::json!({"setupNs": setup_ns, "mainNs": entered.elapsed().as_nanos(), "phases": phases})
        );
    }
    Ok(())
}
