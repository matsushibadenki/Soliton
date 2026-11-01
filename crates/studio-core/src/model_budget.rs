//! Planning payloads, never total resident-memory or admission guarantees.
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, io::Read, path::Path};

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub kv_bytes: Option<u64>,
    pub workspace_bytes: Option<u64>,
    pub kv_basis: Option<&'static str>,
    pub workspace_basis: Option<&'static str>,
    pub retained_kv_limit_bytes: Option<u64>,
    pub allocator_cache_limit_bytes: Option<u64>,
}
fn product(values: &[u64]) -> Option<u64> {
    values.iter().try_fold(1u64, |a, b| a.checked_mul(*b))
}
fn bounded(v: &Value, key: &str, max: u64) -> Option<u64> {
    v[key].as_u64().filter(|n| *n > 0 && *n <= max)
}
fn rounded(n: u64, step: u64) -> Option<u64> {
    n.checked_add(step - 1)?
        .checked_div(step)?
        .checked_mul(step)
}
/// HF dense attention only. Full context for every layer deliberately ignores SWA
/// savings; 256-token growth rounding follows the MLX cache allocation step.
pub fn mlx(v: &Value, context: u64, retained_mib: u32, allocator_mib: u32) -> Budget {
    let mut b = Budget::default();
    if retained_mib != 0 {
        b.retained_kv_limit_bytes = product(&[retained_mib as u64, 1 << 20]);
    }
    if allocator_mib != 0 {
        b.allocator_cache_limit_bytes = product(&[allocator_mib as u64, 1 << 20]);
    }
    let calc = || -> Option<(u64, u64)> {
        if !matches!(v["model_type"].as_str()?, "gemma2" | "gemma3_text")
            || !(1..=131072).contains(&context)
        {
            return None;
        }
        // Unknown dtypes, quantized KV and hybrid layouts must not inherit a default.
        let dtype = v["torch_dtype"].as_str().or_else(|| v["dtype"].as_str())?;
        let bytes = match dtype {
            "float16" | "bfloat16" => 2,
            "float32" => 4,
            _ => return None,
        };
        if v.get("kv_quantization").is_some() || v.get("kv_bits").is_some() {
            return None;
        }
        let layers = bounded(v, "num_hidden_layers", 256)?;
        let heads = bounded(v, "num_key_value_heads", 256)?;
        let qheads = bounded(v, "num_attention_heads", 256)?;
        if heads > qheads || qheads % heads != 0 {
            return None;
        }
        let dim = bounded(v, "head_dim", 1024)?;
        let vocab = bounded(v, "vocab_size", 1_000_000)?;
        Some((
            product(&[2, layers, heads, dim, bytes, rounded(context, 256)?])?,
            product(&[vocab, bytes])?,
        ))
    };
    if let Some((kv, work)) = calc() {
        b.kv_bytes = Some(kv);
        b.kv_basis = Some("dense_payload_envelope");
        // Only one output vector; excludes prefill, graphs, temporaries and allocator.
        b.workspace_bytes = Some(work);
        b.workspace_basis = Some("output_vector_floor");
    }
    b
}

fn u32le(r: &mut impl Read) -> Option<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b).ok()?;
    Some(u32::from_le_bytes(b))
}
fn u64le(r: &mut impl Read) -> Option<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}
fn string(r: &mut impl Read) -> Option<String> {
    let n = u64le(r)?;
    if n > 65536 {
        return None;
    }
    let mut b = vec![0; n as usize];
    r.read_exact(&mut b).ok()?;
    String::from_utf8(b).ok()
}
/// Reads only bounded GGUF metadata, never tensors or the vocabulary array body.
fn gemma_header(path: &Path) -> Option<(BTreeMap<String, u64>, u64)> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let f = std::fs::File::open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut r = f.take(1024 * 1024);
    let mut magic = [0; 4];
    r.read_exact(&mut magic).ok()?;
    if &magic != b"GGUF" || u32le(&mut r)? != 3 {
        return None;
    }
    let _tensors = u64le(&mut r)?;
    let n = u64le(&mut r)?;
    if n > 4096 {
        return None;
    }
    let mut nums = BTreeMap::new();
    let mut arch = false;
    for _ in 0..n {
        let key = string(&mut r)?;
        let ty = u32le(&mut r)?;
        match ty {
            4 => {
                nums.insert(key, u32le(&mut r)? as u64);
            }
            10 => {
                nums.insert(key, u64le(&mut r)?);
            }
            8 => {
                let s = string(&mut r)?;
                if key == "general.architecture" {
                    arch = s == "gemma3";
                }
            }
            9 => {
                let elem = u32le(&mut r)?;
                let count = u64le(&mut r)?;
                if key == "tokenizer.ggml.tokens"
                    && elem == 8
                    && count > 0
                    && count <= 1_000_000
                    && arch
                {
                    return Some((nums, count));
                }
                // Do not traverse arbitrary arrays or guess a partially parsed shape.
                return None;
            }
            0 | 1 | 7 => {
                let mut x = [0; 1];
                r.read_exact(&mut x).ok()?;
            }
            2 | 3 => {
                let mut x = [0; 2];
                r.read_exact(&mut x).ok()?;
            }
            5 | 6 => {
                u32le(&mut r)?;
            }
            11 | 12 => {
                u64le(&mut r)?;
            }
            _ => return None,
        }
    }
    None
}
pub fn gemma(path: &Path, context: u64) -> Budget {
    let calc = || -> Option<(u64, u64)> {
        if !(1..=32768).contains(&context) {
            return None;
        }
        let (n, vocab) = gemma_header(path)?;
        // Restricted to the bundled converter's tested Gemma 3 1B text layout.
        for (key, value) in [
            ("gemma3.block_count", 26),
            ("gemma3.attention.head_count_kv", 1),
            ("gemma3.attention.key_length", 256),
            ("gemma3.attention.value_length", 256),
            ("gemma3.embedding_length", 1152),
            ("gemma3.attention.head_count", 4),
        ] {
            if n.get(key) != Some(&value) {
                return None;
            }
        }
        if vocab != 262144 {
            return None;
        }
        Some((
            product(&[26, 2, 256, 2, rounded(context, 256)?])?,
            product(&[vocab, 4])?,
        ))
    };
    match calc() {
        Some((kv, work)) => Budget {
            kv_bytes: Some(kv),
            workspace_bytes: Some(work),
            kv_basis: Some("dense_payload_envelope"),
            workspace_basis: Some("output_vector_floor"),
            ..Budget::default()
        },
        None => Budget::default(),
    }
}

/// Source-reviewed Qwen Metal window state and selected mandatory scratch buffers.
/// Caller must match the source fingerprint; arena, MTP and prompt GEMM are excluded.
pub fn strata(context: u64) -> Budget {
    let calc = || -> Option<(u64, u64)> {
        if !(512..=262144).contains(&context) {
            return None;
        }
        let cells = ((context.checked_add(64)? / 4) * 4)
            .checked_add(64)?
            .max(4096);
        let pooled = cells / 4 + 2;
        let recurrent = 36u64.checked_mul(4 * 128 * 48 * 128 + 2 * 4 * 10240 * 3)?;
        let per_qsa = product(&[cells, 2, 256, 2])?
            .checked_add(product(&[2, cells, 2, 4, 2])?)?
            .checked_add(4 * 4 * 128 + 4 * 128)?
            .checked_add(product(&[4, pooled, 128])?)?;
        let state = recurrent
            .checked_add(12u64.checked_mul(per_qsa)?)?
            .checked_add(4 * 9 * 10240)?;
        // R_, bo_, xn_, qkv_, z_, qkvL_, zL_: known non-overlapping decode buffers.
        let work = product(&[4, 8, 4, 2560, 2])?
            .checked_add(4 * 8 * 2560)?
            .checked_add(4 * 8 * (10240 + 6144))?
            .checked_add(product(&[4, 36, 8, 10240 + 6144])?)?;
        Some((state, work))
    };
    match calc() {
        Some((kv, work)) => Budget {
            kv_bytes: Some(kv),
            workspace_bytes: Some(work),
            kv_basis: Some("strata_state_payload"),
            workspace_basis: Some("strata_decode_floor"),
            ..Budget::default()
        },
        None => Budget::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mlx_counts_geometry_and_separates_retention() {
        let v = serde_json::json!({"model_type":"gemma2","torch_dtype":"bfloat16","num_hidden_layers":26,"num_attention_heads":8,"num_key_value_heads":4,"head_dim":256,"vocab_size":256000});
        let b = mlx(&v, 4096, 1024, 512);
        assert_eq!(b.kv_bytes, Some(436207616));
        assert_eq!(b.workspace_bytes, Some(512000));
        assert_eq!(b.retained_kv_limit_bytes, Some(1 << 30));
        assert_eq!(b.allocator_cache_limit_bytes, Some(512 << 20));
        assert!(mlx(&serde_json::json!({}), 4096, 0, 0).kv_bytes.is_none());
        let mut bad = v.clone();
        bad["head_dim"] = serde_json::json!(u64::MAX);
        assert!(mlx(&bad, 4096, 0, 0).kv_bytes.is_none());
        bad = v.clone();
        bad["torch_dtype"] = serde_json::json!("int4");
        assert!(mlx(&bad, 4096, 0, 0).kv_bytes.is_none());
        assert!(mlx(&v, 0, 0, 0).kv_bytes.is_none());
        assert!(mlx(&v, u64::MAX, 0, 0).kv_bytes.is_none());
        assert!(mlx(&v, 4097, 0, 0).kv_bytes.unwrap() > b.kv_bytes.unwrap());
    }
    #[test]
    fn strata_context_changes_state_not_decode_floor() {
        let a = strata(8192);
        let b = strata(32768);
        assert_eq!(a.kv_bytes, Some(240715776));
        assert!(b.kv_bytes > a.kv_bytes);
        assert_eq!(a.workspace_bytes, b.workspace_bytes);
        assert!(strata(u64::MAX).kv_bytes.is_none());
    }
    #[test]
    fn gguf_geometry_is_read_without_loading_tensor_data() {
        fn strv(b: &mut Vec<u8>, s: &str) {
            b.extend((s.len() as u64).to_le_bytes());
            b.extend(s.as_bytes());
        }
        let mut b = Vec::from(&b"GGUF"[..]);
        b.extend(3u32.to_le_bytes());
        b.extend(340u64.to_le_bytes());
        b.extend(8u64.to_le_bytes());
        strv(&mut b, "general.architecture");
        b.extend(8u32.to_le_bytes());
        strv(&mut b, "gemma3");
        for (key, n) in [
            ("gemma3.block_count", 26u32),
            ("gemma3.attention.head_count_kv", 1),
            ("gemma3.attention.key_length", 256),
            ("gemma3.attention.value_length", 256),
            ("gemma3.embedding_length", 1152),
            ("gemma3.attention.head_count", 4),
        ] {
            strv(&mut b, key);
            b.extend(4u32.to_le_bytes());
            b.extend(n.to_le_bytes());
        }
        strv(&mut b, "tokenizer.ggml.tokens");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(262144u64.to_le_bytes());
        let path = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&path, &b).unwrap();
        let budget = gemma(&path, 2048);
        assert_eq!(budget.kv_bytes, Some(52 << 20));
        assert_eq!(budget.workspace_bytes, Some(1 << 20));
        assert!(gemma(&path, 32769).kv_bytes.is_none());
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        std::fs::write(&path, &b).unwrap();
        assert!(gemma(&path, 2048).kv_bytes.is_none());
        b[4..8].copy_from_slice(&3u32.to_le_bytes());
        b[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
        std::fs::write(&path, &b).unwrap();
        assert!(gemma(&path, 2048).kv_bytes.is_none());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn malformed_gguf_is_unknown() {
        let path = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&path, b"GGUF").unwrap();
        assert!(gemma(&path, 2048).kv_bytes.is_none());
        std::fs::remove_file(path).unwrap();
    }
}
