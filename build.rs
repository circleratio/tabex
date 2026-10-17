use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let res_dir = manifest_dir.join("res");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Inject the crate version so that Cargo.toml stays the single source of truth.
    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = ver
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse::<u16>().unwrap_or(0).to_string())
        .collect();
    parts.resize(3, "0".to_string());
    parts.push("0".to_string());

    let template = fs::read_to_string(res_dir.join("tabex.rc")).unwrap();
    let rc = template
        .replace("@RES_DIR@", &res_dir.to_string_lossy().replace('\\', "/"))
        .replace("@VER_COMMA@", &parts.join(","))
        .replace("@VER@", &ver);
    let rc_path = out_dir.join("tabex.rc");
    fs::write(&rc_path, rc).unwrap();

    println!("cargo:rerun-if-changed=res/tabex.rc");
    println!("cargo:rerun-if-changed=res/tabex.manifest");
    println!("cargo:rerun-if-changed=res/tabex.ico");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
