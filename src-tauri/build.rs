use std::{env, path::PathBuf, process::Command};

fn main() {
    for key in [
        "PIXEL_CANDIDATE_MANIFEST",
        "PIXEL_BUILD_ID",
        "PIXEL_BUILD_CHANNEL",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let values = if let Ok(manifest) = env::var("PIXEL_CANDIDATE_MANIFEST") {
        println!("cargo:rerun-if-changed={manifest}");
        let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
            .parent()
            .unwrap()
            .to_path_buf();
        let output = Command::new("node")
            .arg(root.join("scripts/candidate.mjs"))
            .arg("identity-values")
            .current_dir(&root)
            .output()
            .expect("Node is required to validate a frozen candidate");
        assert!(
            output.status.success(),
            "Candidate validation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("Candidate identity must be UTF-8");
        let fields: Vec<String> = text.lines().map(str::to_owned).collect();
        assert_eq!(fields.len(), 3, "Candidate identity response is incomplete");
        fields
    } else {
        assert!(
            env::var_os("PIXEL_BUILD_ID").is_none(),
            "A build ID requires a candidate manifest"
        );
        vec![String::new(), "local-native".into(), String::new()]
    };
    for (key, value) in [
        "PIXEL_CANDIDATE_ID",
        "PIXEL_COMPILED_BUILD_ID",
        "PIXEL_SOURCE_FINGERPRINT",
    ]
    .iter()
    .zip(values)
    {
        assert!(!value.contains(['\n', '\r']), "Invalid build identity");
        println!("cargo:rustc-env={key}={value}");
    }
    tauri_build::build()
}
