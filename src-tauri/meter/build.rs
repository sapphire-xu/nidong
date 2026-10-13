use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

fn main() {
    // 调试构建不嵌入。cargo test 若继承这份清单，会因需要提权而起不来。
    let profile = env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = out.join("app.manifest");
    let rc_path = out.join("app.rc");
    let res_path = out.join("app.res");
    fs::write(&manifest, MANIFEST).unwrap();
    let manifest_for_rc = manifest.display().to_string().replace('\\', "/");
    fs::write(&rc_path, format!("1 24 \"{manifest_for_rc}\"\n")).unwrap();

    let rc = find_rc();
    let output = Command::new(&rc)
        .arg("/nologo")
        .arg(format!("/fo{}", res_path.display()))
        .arg(&rc_path)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {}: {err}", rc.display()));
    if !output.status.success() {
        panic!("rc.exe failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    println!("cargo:rustc-link-arg-bin=nidong-meter={}", res_path.display());
    println!("cargo:rerun-if-changed=build.rs");
}

fn find_rc() -> PathBuf {
    let roots = [
        PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin"),
        PathBuf::from(r"C:\Program Files\Windows Kits\10\bin"),
    ];
    let mut found = Vec::new();
    for root in roots {
        let Ok(versions) = fs::read_dir(&root) else {
            continue;
        };
        for version in versions.flatten() {
            let candidate = version.path().join("x64").join("rc.exe");
            if candidate.is_file() {
                found.push(candidate);
            }
        }
    }
    found.sort();
    found.pop().expect("rc.exe was not found under the Windows SDK")
}
