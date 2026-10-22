//! Embeds the Windows manifest into crest.exe (see packaging/crest.exe.manifest), along with its
//! icon and version info (the "Crest" name in Task Manager and file properties).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.join("packaging").join("crest.exe.manifest");
    println!("cargo:rerun-if-changed=packaging/crest.exe.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());

    // Icon and version via rc.exe from the Windows SDK, which comes with the MSVC build tools.
    // Without rc.exe, crest.exe is still built, just without an icon.
    println!("cargo:rerun-if-changed=packaging/crest.ico");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe (Windows SDK) not found; building crest.exe without an icon");
        return;
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let script = out.join("crest.rc");
    let resources = out.join("crest.res");
    std::fs::write(&script, resource_script(&root.join("packaging").join("crest.ico"))).expect("crest.rc");
    let built = Command::new(rc).arg("/nologo").arg("/fo").arg(&resources).arg(&script).status();
    if built.is_ok_and(|status| status.success()) {
        println!("cargo:rustc-link-arg-bins={}", resources.display());
    } else {
        println!("cargo:warning=failed to build the crest.exe icon");
    }
}

fn resource_script(icon: &Path) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let numbers: Vec<&str> = version.split('.').chain(["0"; 4]).take(4).collect();
    let numbers = numbers.join(",");
    let icon = icon.display().to_string().replace('\\', "\\\\");
    format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Rinci Labs"
      VALUE "FileDescription", "Crest"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "crest"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "crest.exe"
      VALUE "ProductName", "Crest"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// rc.exe from the newest installed Windows SDK.
fn find_rc() -> Option<PathBuf> {
    let kits = PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits").join("10").join("bin");
    let mut found: Vec<PathBuf> = std::fs::read_dir(kits)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path().join("x64").join("rc.exe")))
        .filter(|rc| rc.exists())
        .collect();
    found.sort();
    found.pop()
}
