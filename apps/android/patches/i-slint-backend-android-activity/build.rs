// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore Dexer multidex
use std::env;
use std::path::PathBuf;

use android_build::{Dexer, JavaBuild};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(slint_nightly_test)");
    if !env::var("TARGET").unwrap().contains("android") {
        return;
    }
    let release_mode = env::var("PROFILE").as_ref().map(|s| s.as_str()) == Ok("release");

    // This is the only Java source file
    let java_src = "SlintAndroidJavaHelper.java";
    let java_src_path = format!("java/{java_src}");

    let slint_path: PathBuf = ["dev", "slint", "android-activity"].iter().collect();

    let out_dir: PathBuf = env::var_os("OUT_DIR").unwrap().into();
    let mut out_class_dir = out_dir.clone();
    out_class_dir.push("java");
    out_class_dir.push(slint_path);

    if out_class_dir.try_exists().unwrap_or(false) {
        let _ = std::fs::remove_dir_all(&out_class_dir);
    }
    std::fs::create_dir_all(&out_class_dir)
        .unwrap_or_else(|e| panic!("Cannot create output directory {out_class_dir:?} - {e}"));

    let android_jar = android_build::android_jar(None).expect("No Android platforms found");

    // Compile the Java file into .class files
    let o = JavaBuild::new()
        .file(&java_src_path)
        .class_path(&android_jar)
        .classes_out_dir(&out_class_dir)
        .java_source_version(8)
        .java_target_version(8)
        .debug_info(android_build::DebugInfo {
            line_numbers: !release_mode,
            variables: !release_mode,
            source_files: !release_mode,
        })
        .command()
        .unwrap_or_else(|e| panic!("Could not generate the java compiler command: {e}"))
        .args(["-encoding", "UTF-8"])
        .output()
        .unwrap_or_else(|e| panic!("Could not run the java compiler: {e}"));

    if !o.status.success() {
        panic!("Java compilation failed: {}", String::from_utf8_lossy(&o.stderr));
    }

    // RustyPresenter patch (see PATCHED.md): JDK 21 and newer write a `MethodParameters` attribute with unnamed
    // parameters for inner-class constructors, which the d8 of build-tools 34 and older cannot read (Slint issue
    // 4973). The helper never reads its parameters' names, so the attribute is taken out before dexing.
    strip_method_parameters(&out_class_dir);

    let o = Dexer::new()
        .android_jar(&android_jar)
        .class_path(&out_class_dir)
        .collect_classes(&out_class_dir)
        .unwrap()
        .release(release_mode)
        .android_min_api(20) // disable multidex for single dex file output
        .out_dir(out_dir)
        .command()
        .unwrap_or_else(|e| panic!("Could not generate the D8 command: {e}"))
        .output()
        .unwrap_or_else(|e| panic!("Error running D8: {e}"));

    if !o.status.success() {
        eprintln!("Dex conversion failed: {}", String::from_utf8_lossy(&o.stderr));
        let java_home = android_build::java_home().unwrap();
        let java_ver = android_build::check_javac_version(&java_home).unwrap();
        if java_ver >= 21 {
            eprintln!("WARNING: JDK version 21 is known to cause an error with older android SDK");
            eprintln!("See https://github.com/slint-ui/slint/issues/4973");
            eprintln!(
                "Try downgrading your version of Java to something like JDK 17, or upgrade to the SDK build tools 35"
            );
        }
        panic!("Dex conversion failed");
    }

    println!("cargo:rerun-if-changed={java_src_path}");
}

/// Every `.class` file under `dir` without its methods' `MethodParameters` attributes.
fn strip_method_parameters(dir: &std::path::Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() { strip_method_parameters(&path); continue }
        if path.extension().is_some_and(|e| e == "class") {
            let bytes = std::fs::read(&path).expect("read a class file");
            if let Some(out) = without_method_parameters(&bytes) {
                std::fs::write(&path, out).expect("write a class file");
            }
        }
    }
}

/// A class file (JVM spec, chapter 4) with the `MethodParameters` attributes of its methods left out; None when it
/// cannot be read (it is then left as it is).
fn without_method_parameters(b: &[u8]) -> Option<Vec<u8>> {
    let u2 = |at: usize| -> Option<usize> { Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize) };
    let u4 = |at: usize| -> Option<usize> { Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]) as usize) };
    if b.get(..4)? != [0xCA, 0xFE, 0xBA, 0xBE] { return None }
    // The constant pool: where each UTF-8 entry is, to know the attributes' names.
    let count = u2(8)?;
    let mut utf8 = vec![None; count];
    let mut at = 10;
    let mut i = 1;
    while i < count {
        let tag = *b.get(at)?;
        let len = match tag {
            1 => { let n = u2(at + 1)?; utf8[i] = Some(b.get(at + 3..at + 3 + n)?); 3 + n }
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => 5,
            5 | 6 => 9,
            7 | 8 | 16 | 19 | 20 => 3,
            15 => 4,
            _ => return None,
        };
        at += len;
        i += if tag == 5 || tag == 6 { 2 } else { 1 };
    }
    let is_mp = |name: usize| utf8.get(name).copied().flatten() == Some(b"MethodParameters".as_slice());
    let mut out = b[..at].to_vec();
    // Access flags, this class, its parent, then the interfaces.
    let interfaces = u2(at + 6)?;
    let head = 8 + 2 * interfaces;
    out.extend_from_slice(b.get(at..at + head)?);
    at += head;
    // Fields as they are; methods without the attribute.
    for strip in [false, true] {
        let n = u2(at)?;
        out.extend_from_slice(b.get(at..at + 2)?);
        at += 2;
        for _ in 0..n {
            out.extend_from_slice(b.get(at..at + 6)?);
            let attrs = u2(at + 6)?;
            at += 8;
            let mut kept = Vec::new();
            let mut kept_n = 0u16;
            for _ in 0..attrs {
                let len = 6 + u4(at + 2)?;
                if !(strip && is_mp(u2(at)?)) { kept.extend_from_slice(b.get(at..at + len)?); kept_n += 1 }
                at += len;
            }
            out.extend_from_slice(&kept_n.to_be_bytes());
            out.extend(kept);
        }
    }
    // The class's own attributes.
    out.extend_from_slice(b.get(at..)?);
    Some(out)
}
