//! C5: pptx -> (LibreOffice headless) -> PDF -> (pdfium) -> slide-NNN.png at 1920 px wide.
//!
//! Usage:
//!   c5-libreoffice convert <in.pptx> <outdir> [--soffice <path>] [--timeout <secs>] [--profile <dir>]
//!   c5-libreoffice verify <outdir> <rrggbb> [<rrggbb> ...]
//! Env: PDFIUM_DLL (default %LOCALAPPDATA%\RustyPresenter\deps\pdfium\bin\pdfium.dll)

use pdfium_render::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TARGET_WIDTH: i32 = 1920;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("convert") => convert(&args[1..]),
        Some("verify") => verify(&args[1..]),
        _ => Err("usage: c5-libreoffice convert <in.pptx> <outdir> [--soffice <path>] [--timeout <secs>] | verify <outdir> <rrggbb>...".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}

fn find_soffice(explicit: Option<&str>) -> Result<PathBuf, String> {
    if let Some(p) = explicit {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!("LibreOffice (soffice) not found at {}", p.display()))
        };
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(pf) = std::env::var(var) {
                // soffice.com is the console launcher: it waits for the conversion and forwards output.
                let prog = Path::new(&pf).join("LibreOffice").join("program");
                candidates.push(prog.join("soffice.com"));
                candidates.push(prog.join("soffice.exe"));
            }
        }
    } else if cfg!(target_os = "macos") {
        candidates.push("/Applications/LibreOffice.app/Contents/MacOS/soffice".into());
    } else {
        for p in [
            "/usr/bin/soffice",
            "/usr/bin/libreoffice",
            "/usr/local/bin/soffice",
            "/opt/libreoffice/program/soffice",
            "/snap/bin/libreoffice",
        ] {
            candidates.push(p.into());
        }
    }
    candidates.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        let list: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
        format!("LibreOffice (soffice) not found; looked in: {}", list.join(", "))
    })
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

fn kill_tree(pid: u32) {
    if cfg!(windows) {
        // soffice.com starts soffice.exe -> soffice.bin as children; kill the whole tree.
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

fn convert(args: &[String]) -> Result<(), String> {
    let mut pos = Vec::new();
    let mut soffice_arg = None;
    let mut profile_dir: Option<PathBuf> = None;
    let mut timeout = Duration::from_secs(120);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--soffice" => soffice_arg = it.next().cloned(),
            "--profile" => profile_dir = it.next().map(PathBuf::from),
            "--timeout" => {
                let secs = it.next().and_then(|s| s.parse().ok()).ok_or("--timeout needs seconds")?;
                timeout = Duration::from_secs(secs);
            }
            _ => pos.push(a.clone()),
        }
    }
    let [input, outdir] = pos.as_slice() else {
        return Err("convert needs <in.pptx> <outdir>".into());
    };
    let input = PathBuf::from(input);
    if !input.is_file() {
        return Err(format!("input file not found: {}", input.display()));
    }
    let input = std::fs::canonicalize(&input).map_err(|e| format!("input {}: {e}", input.display()))?;
    // canonicalize gives \\?\C:\... on Windows; LibreOffice does not accept that prefix.
    let input = PathBuf::from(input.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    let outdir = PathBuf::from(outdir);
    let soffice = find_soffice(soffice_arg.as_deref())?;
    println!("soffice: {}", soffice.display());

    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let work = std::env::temp_dir().join(format!("c5-{}-{stamp}", std::process::id()));
    // Default: a fresh throwaway profile per run. --profile <dir> keeps one between runs (what the app would do).
    let profile = profile_dir.unwrap_or_else(|| work.join("profile"));
    let pdf_dir = work.join("pdf");
    std::fs::create_dir_all(&pdf_dir).map_err(|e| format!("temp dir: {e}"))?;
    let profile_arg = format!("-env:UserInstallation={}", file_url(&profile));

    // ---- conversion ----
    let t0 = Instant::now();
    let mut child = Command::new(&soffice)
        .args(["--headless", "--norestore", "--nologo", "--nodefault", "--nolockcheck"])
        .arg(&profile_arg)
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(&pdf_dir)
        .arg(&input)
        // Git Bash / other Pythons leak these; LibreOffice's bundled Python then warns.
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", soffice.display()))?;
    let status = loop {
        match child.try_wait().map_err(|e| format!("waiting for soffice: {e}"))? {
            Some(s) => break s,
            None if t0.elapsed() >= timeout => {
                kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_dir_all(&work);
                return Err(format!(
                    "conversion timed out after {} s; LibreOffice was killed",
                    timeout.as_secs()
                ));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let conv_ms = t0.elapsed().as_millis();
    let pdf = pdf_dir.join(input.with_extension("pdf").file_name().unwrap());
    if !pdf.is_file() {
        let _ = std::fs::remove_dir_all(&work);
        return Err(format!(
            "LibreOffice produced no PDF ({status}); expected {}",
            pdf.display()
        ));
    }
    let pdf_len = std::fs::metadata(&pdf).map(|m| m.len()).unwrap_or(0);
    println!("conversion: {conv_ms} ms ({status}) -> {} ({pdf_len} bytes)", pdf.display());

    // ---- render ----
    let t1 = Instant::now();
    let dll = std::env::var("PDFIUM_DLL").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join(r"RustyPresenter\deps\pdfium\bin\pdfium.dll")
    });
    let bindings = Pdfium::bind_to_library(&dll)
        .map_err(|e| format!("could not load pdfium from {}: {e}", dll.display()))?;
    let pdfium = Pdfium::new(bindings);
    let load_ms = t1.elapsed().as_millis();
    let doc = pdfium
        .load_pdf_from_file(&pdf, None)
        .map_err(|e| format!("pdfium could not open PDF: {e}"))?;
    std::fs::create_dir_all(&outdir).map_err(|e| format!("outdir: {e}"))?;
    let cfg = PdfRenderConfig::new().set_target_width(TARGET_WIDTH);
    let n = doc.pages().len();
    for (i, page) in doc.pages().iter().enumerate() {
        let tp = Instant::now();
        let (wpt, hpt) = (page.width().value, page.height().value);
        let img = page
            .render_with_config(&cfg)
            .map_err(|e| format!("render page {}: {e}", i + 1))?
            .as_image();
        let file = outdir.join(format!("slide-{:03}.png", i + 1));
        img.save(&file).map_err(|e| format!("save {}: {e}", file.display()))?;
        println!(
            "page {}: {:.1}x{:.1} pt (ratio {:.4}) -> {}x{} px, {} ms -> {}",
            i + 1,
            wpt,
            hpt,
            wpt / hpt,
            img.width(),
            img.height(),
            tp.elapsed().as_millis(),
            file.display()
        );
    }
    let render_ms = t1.elapsed().as_millis();
    println!("render: {n} pages in {render_ms} ms (pdfium load {load_ms} ms)");
    println!("TOTAL: conversion {conv_ms} ms + render {render_ms} ms");
    drop(doc);
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

fn parse_hex(s: &str) -> Result<[u8; 3], String> {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).map_err(|_| format!("bad colour {s}"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn verify(args: &[String]) -> Result<(), String> {
    let (dir, colours) = args.split_first().ok_or("verify needs <outdir> <rrggbb>...")?;
    let expected: Vec<[u8; 3]> = colours.iter().map(|c| parse_hex(c)).collect::<Result<_, _>>()?;
    let mut pngs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    pngs.sort();
    let mut ok = true;
    println!("{} PNGs found, {} expected", pngs.len(), expected.len());
    if pngs.len() != expected.len() {
        ok = false;
    }
    for (p, exp) in pngs.iter().zip(&expected) {
        let img = image::open(p).map_err(|e| format!("{}: {e}", p.display()))?.to_rgb8();
        let (w, h) = img.dimensions();
        let aspect = match h {
            1080 => "16:9",
            1440 => "4:3",
            _ => "other",
        };
        // Dominant colour of a 160x160 block in the top-left margin (text is centred, so this is background).
        let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
        for y in 8..168.min(h) {
            for x in 8..168.min(w) {
                *counts.entry(img.get_pixel(x, y).0).or_default() += 1;
            }
        }
        let (dom, cnt) = counts.into_iter().max_by_key(|(_, c)| *c).unwrap();
        let close = dom.iter().zip(exp).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 20);
        let size_ok = w == 1920 && (h == 1080 || h == 1440);
        println!(
            "{}: {w}x{h} ({aspect}) dominant #{:02x}{:02x}{:02x} ({cnt}/25600 px), expected #{:02x}{:02x}{:02x} -> {}",
            p.file_name().unwrap().to_string_lossy(),
            dom[0],
            dom[1],
            dom[2],
            exp[0],
            exp[1],
            exp[2],
            if close && size_ok { "ok" } else { "MISMATCH" }
        );
        ok &= close && size_ok;
    }
    if ok {
        println!("VERIFY PASS");
        Ok(())
    } else {
        Err("verification failed".into())
    }
}
