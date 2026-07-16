use maku_bench::{generate, load_workload};
use serde_json::json;
use std::path::{Path, PathBuf};

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let workload = PathBuf::from(args.next().ok_or("usage: maku-bench-generate WORKLOAD OUTPUT_DIR")?);
    let output = PathBuf::from(args.next().ok_or("usage: maku-bench-generate WORKLOAD OUTPUT_DIR")?);
    if args.next().is_some() { return Err("too many arguments".into()); }
    let spec = load_workload(&workload)?;
    let fixture = generate(&spec)?;
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let source_path = output.join(format!("{}.expanded.maku", spec.id));
    let metadata_path = output.join(format!("{}.fixture.json", spec.id));
    std::fs::write(&source_path, &fixture.source).map_err(|e| e.to_string())?;
    let workload_rel = workload.strip_prefix(Path::new(".")).unwrap_or(&workload);
    let metadata = json!({
        "fixture_schema_version": 1,
        "id": spec.id,
        "generator": spec.generator_version,
        "workload": workload_rel,
        "workload_sha256": fixture.workload_sha256,
        "expanded_source": source_path,
        "expanded_source_sha256": fixture.source_sha256,
        "input_tape_sha256": fixture.input_tape_sha256,
        "seed": spec.seed,
        "expect": spec.expect,
    });
    std::fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata).unwrap()).map_err(|e| e.to_string())?;
    println!("{}  {}", fixture.source_sha256, source_path.display());
    Ok(())
}
