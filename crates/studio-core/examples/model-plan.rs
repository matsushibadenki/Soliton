//! Read-only metadata diagnostic; does not launch an engine or load model weights.
use std::path::PathBuf;
use studio_core::{engine::Engine, model_plan, Settings};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 3 || !matches!(args[0].as_str(), "strata" | "mlx") {
        eprintln!("usage: model-plan strata ROOT CONFIG | model-plan mlx MODEL_DIRECTORY CONTEXT");
        std::process::exit(2);
    }
    let mut s = Settings::default();
    if args[0] == "strata" {
        s.strata_root = args[1].clone();
        s.config = args[2].clone();
    } else {
        s.engine = Engine::VllmApple;
        s.vllm.model = args[1].clone();
        s.vllm.context = args[2].parse().expect("positive context");
    }
    let before = if args[0] == "strata" {
        std::fs::read(PathBuf::from(&s.strata_root).join(&s.config)).ok()
    } else {
        None
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&model_plan::inspect(
            &s,
            Some(32 << 30),
            &serde_json::Value::Null
        ))
        .unwrap()
    );
    if let Some(before) = before {
        assert_eq!(
            std::fs::read(PathBuf::from(&s.strata_root).join(&s.config)).unwrap(),
            before
        );
    }
}
