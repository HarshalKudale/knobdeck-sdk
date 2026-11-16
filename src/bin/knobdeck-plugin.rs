//! `knobdeck-plugin` — CLI for Rust KnobDeck plugins (run from the plugin crate folder).
//!
//!   new <folder> [--name N] [--id ID] [--os windows|macos|linux] [--author A] [--description D]
//!                                  create a plugin crate (with the AI agent skill)
//!   skill [folder]                 install the agent skill (default .claude/skills/knobdeck-plugin)
//!   drive [steps.json]             run dist/<id>/ over the real protocol without KnobDeck
//!   dev [--skip-setup] [--watch]   cargo build (debug), assemble dist/<id>/ for
//!                                  "Load Unpacked"; --watch rebuilds on change and
//!                                  KnobDeck restarts the plugin when its exe changes
//!   build                          cargo build --release, assemble dist/<id>/
//!                                  (on macOS: universal arm64 + x86_64 binaries via lipo)
//!   package                        zip dist/<id>/ into dist/<id>.knobdeck
//!
//! Codegen runs every built binary with KNOBDECK_CODEGEN=1; the plugin binary
//! prints { manifest, actions } and the setup binary exits silently.

#[path = "knobdeck-plugin/drive.rs"]
mod drive;
#[path = "knobdeck-plugin/conform.rs"]
mod conform;
#[path = "knobdeck-plugin/scaffold.rs"]
mod scaffold;

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const USAGE: &str = "knobdeck-plugin <new|dev|drive|conform|build|package|skill>

  new <folder> [--name N] [--id ID] [--os windows|macos|linux]
               [--author A] [--description D]
                                create a plugin crate, with the AI agent skill
  dev [--skip-setup] [--watch]  build (debug) + assemble dist/<id>/ for \"Load Unpacked\"
  drive [steps.json]            run dist/<id>/ over the real protocol, without KnobDeck
  conform [--dir D] [-- CMD..]  check that a plugin (any language) follows the protocol
  build                         build (release) + assemble dist/<id>/
  package                       zip dist/<id>/ -> dist/<id>.knobdeck
  skill [folder]                write the agent skill (default .claude/skills/knobdeck-plugin)
";

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("\n  ✗ {msg}");
    std::process::exit(1)
}

fn copy_dir(src: &Path, dest: &Path) {
    if !src.is_dir() {
        return;
    }
    fs::create_dir_all(dest).unwrap_or_else(|e| die(e));
    for entry in fs::read_dir(src).unwrap_or_else(|e| die(e)).flatten() {
        let (s, d) = (entry.path(), dest.join(entry.file_name()));
        if s.is_dir() {
            copy_dir(&s, &d)
        } else {
            fs::copy(&s, &d).unwrap_or_else(|e| die(format!("copy {}: {e}", s.display())));
        }
    }
}

/// Release builds on macOS are universal (Apple silicon + Intel), like the KnobDeck app.
fn build(release: bool) -> Vec<PathBuf> {
    if release && cfg!(target_os = "macos") {
        universal_build()
    } else {
        cargo_build(release, None)
    }
}

/// Build for both Mac architectures and `lipo` each binary into target/universal-apple-darwin/release/.
fn universal_build() -> Vec<PathBuf> {
    let arm = cargo_build(true, Some("aarch64-apple-darwin"));
    let x64 = cargo_build(true, Some("x86_64-apple-darwin"));
    let out_dir = arm
        .first()
        .and_then(|e| e.parent()?.parent()?.parent())
        .unwrap_or_else(|| die("cargo produced no executables"))
        .join("universal-apple-darwin/release");
    fs::create_dir_all(&out_dir).unwrap_or_else(|e| die(e));
    arm.iter()
        .map(|a| {
            let name = a.file_name().unwrap();
            let x = x64.iter().find(|x| x.file_name() == Some(name)).unwrap_or_else(|| die(format!("no x86_64 build of {}", name.to_string_lossy())));
            let out = out_dir.join(name);
            let ok = Command::new("lipo").arg("-create").arg("-output").arg(&out).arg(a).arg(x).status().map_or(false, |s| s.success());
            if !ok {
                die(format!("lipo failed for {}", name.to_string_lossy()));
            }
            out
        })
        .collect()
}

/// `cargo build`, returning every produced executable.
fn cargo_build(release: bool, target: Option<&str>) -> Vec<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--message-format=json-render-diagnostics"]);
    if release {
        cmd.arg("--release");
    }
    if let Some(t) = target {
        cmd.args(["--target", t]);
    }
    let out = cmd.stderr(Stdio::inherit()).output().unwrap_or_else(|e| die(format!("cargo: {e}")));
    if !out.status.success() {
        match target {
            Some(t) => die(format!("cargo build --target {t} failed (missing target? run `rustup target add {t}`)")),
            None => die("cargo build failed"),
        }
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact")
        .filter_map(|m| m["executable"].as_str().map(PathBuf::from))
        .collect()
}

/// Run each binary in codegen mode; the one that prints a manifest is the plugin.
fn codegen(exes: &[PathBuf], stage: &str) -> (PathBuf, Value) {
    for exe in exes {
        let out = Command::new(exe)
            .env("KNOBDECK_CODEGEN", "1")
            .env("KNOBDECK_STAGE", stage)
            .stderr(Stdio::inherit())
            .output()
            .unwrap_or_else(|e| die(format!("{}: {e}", exe.display())));
        if !out.status.success() {
            die(format!("codegen failed for {}", exe.display()));
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
            if v["manifest"].is_object() {
                return (exe.clone(), v);
            }
        }
    }
    die("no binary produced a manifest — does main() call Plugin::run()?")
}

fn write_json(path: PathBuf, v: &Value) {
    fs::write(&path, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap_or_else(|e| die(format!("{}: {e}", path.display())));
}

/// Copy the setup page (+ assets dir) into dist/<id>/setup-ui and point the manifest at it.
fn stage_setup_ui(manifest: &mut Value, meta: &Value, dist: &Path) {
    let Some(page) = meta["setup_page"].as_str() else { return };
    let page = Path::new(page);
    if !page.is_file() {
        die(format!("Setup page \"{}\" not found.", page.display()));
    }
    let ui = dist.join("setup-ui");
    fs::create_dir_all(&ui).unwrap_or_else(|e| die(e));
    let name = page.file_name().unwrap();
    fs::copy(page, ui.join(name)).unwrap_or_else(|e| die(e));
    if let Some(assets) = meta["setup_assets_dir"].as_str() {
        copy_dir(Path::new(assets), &ui);
    }
    manifest["setup"]["page"] = format!("setup-ui/{}", name.to_string_lossy()).into();
}

/// Copy a built binary into `dist`. KnobDeck may be running the previous copy,
/// which Windows won't let us overwrite but will let us rename, so move it aside.
fn stage_exe(exe: &Path, dist: &Path) {
    let dest = dist.join(exe.file_name().unwrap());
    if dest.exists() {
        let old = dest.with_extension("old");
        let _ = fs::remove_file(&old);
        let _ = fs::rename(&dest, &old);
    }
    fs::copy(exe, &dest).unwrap_or_else(|e| die(format!("copy {}: {e}", exe.display())));
}

/// Keep the other OSes' `executables` entries from a previous build in the same
/// `dist/<id>/` (e.g. built on Windows, then on macOS), as long as their binaries
/// are still there, so one package runs on both.
fn merge_other_os_executables(manifest: &mut Value, dist: &Path) {
    let Ok(text) = fs::read_to_string(dist.join("manifest.json")) else { return };
    let Ok(old) = serde_json::from_str::<Value>(&text) else { return };
    for section in ["runtime", "setup"] {
        // `get_mut`, not `manifest[section][..]`: IndexMut on a Value inserts
        // missing keys as null, which would add a bogus `"setup": {"executables": null}`.
        let cur = manifest.get_mut(section).and_then(|s| s.get_mut("executables")).and_then(Value::as_object_mut);
        let (Some(prev), Some(cur)) = (old[section]["executables"].as_object(), cur) else {
            continue;
        };
        for (os, exe) in prev {
            let present = exe.as_str().is_some_and(|e| dist.join(e).is_file());
            if !cur.contains_key(os) && present {
                cur.insert(os.clone(), exe.clone());
                println!("  · keeping {section} executable for {os}: {}", exe.as_str().unwrap_or_default());
            }
        }
    }
}

fn assemble(release: bool, skip_setup: bool) {
    // Both stages ship the binaries inside dist/<id>/: running them out of
    // target/ would lock them against the next cargo build.
    let stage = "build";
    println!("\n  KnobDeck Plugin — {}", if release { "build" } else { "dev" });
    let exes = build(release);
    let (plugin_exe, mut out) = codegen(&exes, stage);
    let mut manifest = out["manifest"].take();
    let actions = out["actions"].take();
    let id = manifest["id"].as_str().unwrap_or_else(|| die("manifest has no id")).to_string();

    let dist = Path::new("dist").join(&id);
    fs::create_dir_all(&dist).unwrap_or_else(|e| die(e));
    if skip_setup {
        manifest["setup_required"] = false.into();
    }
    stage_setup_ui(&mut manifest, &out["meta"], &dist);
    copy_dir(Path::new("icons"), &dist.join("icons"));

    // Self-contained: ship the plugin binary and (if declared) the setup binary.
    let mut ship = vec![plugin_exe.clone()];
    if let Some(setup) = manifest["setup"]["executable"].as_str() {
        match exes.iter().find(|e| e.file_name().map_or(false, |n| n == Path::new(setup).file_name().unwrap())) {
            Some(e) => ship.push(e.clone()),
            None => die(format!("setup binary \"{setup}\" was not built — add a [[bin]] for it")),
        }
    }
    for exe in ship {
        stage_exe(&exe, &dist);
    }
    if release {
        for readme in ["readme.md", "README.md"] {
            if Path::new(readme).is_file() {
                fs::copy(readme, dist.join("readme.md")).ok();
                break;
            }
        }
    }

    merge_other_os_executables(&mut manifest, &dist);
    write_json(dist.join("manifest.json"), &manifest);
    write_json(dist.join("actions.json"), &actions);
    println!("  ✓ dist/{id}/ written{}", if release { "\n  Run \"knobdeck-plugin package\" to produce a .knobdeck file." } else { "" });
}

fn package() {
    let dirs: Vec<PathBuf> = fs::read_dir("dist")
        .unwrap_or_else(|_| die("no dist/ — run `knobdeck-plugin build` first"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    let dir = match dirs.as_slice() {
        [d] => d.clone(),
        [] => die("no dist/<id>/manifest.json — run `knobdeck-plugin build` first"),
        _ => die("multiple plugins under dist/ — remove the stale ones"),
    };
    let out = Path::new("dist").join(format!("{}.knobdeck", dir.file_name().unwrap().to_string_lossy()));
    // bsdtar writes zip with --format zip. It is `tar` on Windows 10+ and macOS; Linux's `tar` is
    // GNU tar (no zip), so use `bsdtar` there (Debian/Ubuntu: libarchive-tools; Arch: libarchive).
    let tar = if Command::new("bsdtar").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok() { "bsdtar" } else { "tar" };
    let status = Command::new(tar)
        .args(["--format", "zip", "-cf"])
        .arg(&out)
        .arg("-C")
        .arg(&dir)
        // Top-level names (no "./" prefix); skip binaries `dev` moved aside.
        .args(
            fs::read_dir(&dir)
                .unwrap_or_else(|e| die(e))
                .flatten()
                .map(|e| e.file_name())
                .filter(|n| Path::new(n).extension().map_or(true, |x| x != "old")),
        )
        .status()
        .unwrap_or_else(|e| die(format!("{tar}: {e}")));
    if !status.success() {
        if cfg!(target_os = "linux") && tar == "tar" {
            die("tar failed to create the archive: install bsdtar (Debian/Ubuntu: libarchive-tools, Arch: libarchive)");
        }
        die(format!("{tar} failed to create the archive"));
    }
    println!("  ✓ {}", out.display());
}

/// Newest modification time under `path` (recursive).
fn newest_mtime(path: &Path) -> Option<std::time::SystemTime> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_dir() {
        return meta.modified().ok();
    }
    fs::read_dir(path).ok()?.flatten().filter_map(|e| newest_mtime(&e.path())).max()
}

/// Rebuild (`dev`) whenever src/, icons/, setup-ui/ or Cargo.toml change. Each
/// rebuild runs as a child so a compile error doesn't end the watch.
fn watch(skip_setup: bool) -> ! {
    let exe = std::env::current_exe().unwrap_or_else(|e| die(e));
    let mut last = None;
    loop {
        let stamp = ["src", "icons", "setup-ui", "Cargo.toml"].iter().filter_map(|p| newest_mtime(Path::new(p))).max();
        if stamp != last {
            last = stamp;
            let mut cmd = Command::new(&exe);
            cmd.arg("dev");
            if skip_setup {
                cmd.arg("--skip-setup");
            }
            let _ = cmd.status();
            println!("  … watching for changes (Ctrl+C to stop)");
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// `new <folder> [--flag value]…`
fn new_plugin(args: &[String]) {
    let mut dir = None;
    let mut opt = std::collections::HashMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.strip_prefix("--") {
            Some(flag @ ("name" | "id" | "os" | "author" | "description")) => {
                let v = it.next().unwrap_or_else(|| die(format!("--{flag} needs a value")));
                opt.insert(flag, v.clone());
            }
            Some(other) => die(format!("unknown option --{other}\n\n{USAGE}")),
            None if dir.is_none() => dir = Some(PathBuf::from(a)),
            None => die(format!("unexpected argument {a:?}")),
        }
    }
    let dir = dir.unwrap_or_else(|| die(format!("which folder?\n\n{USAGE}")));
    let o = scaffold::NewOptions {
        dir: dir.clone(),
        id: opt.remove("id"),
        name: opt.remove("name"),
        os: opt.remove("os"),
        author: opt.remove("author"),
        description: opt.remove("description"),
    };
    let notes = scaffold::new_plugin(&o).unwrap_or_else(|e| die(e));
    println!("\n  ✓ created {}", dir.display());
    for n in notes {
        println!("  · {n}");
    }
    println!("\n  Next:\n    cd {}\n    knobdeck-plugin dev      # then \"Load Unpacked\" dist/<id>/ in KnobDeck\n    knobdeck-plugin drive    # smoke test without KnobDeck\n", dir.display());
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    match args.first().map(String::as_str) {
        Some("dev") if flag("--watch") => watch(flag("--skip-setup")),
        Some("dev") => assemble(false, flag("--skip-setup")),
        Some("build") => assemble(true, false),
        Some("package") => package(),
        Some("drive") => drive::drive(args.get(1).map(String::as_str)).unwrap_or_else(|e| die(e)),
        Some("conform") => conform::conform(&args[1..]).unwrap_or_else(|e| die(e)),
        Some("skill") => {
            let dir = args.get(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".claude/skills/knobdeck-plugin"));
            scaffold::install_skill(&dir).unwrap_or_else(|e| die(e));
            println!("  ✓ agent skill written to {}", dir.display());
        }
        Some("new") => new_plugin(&args[1..]),
        _ => print!("{USAGE}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_keeps_other_os_and_never_invents_setup() {
        let dist = std::env::temp_dir().join(format!("knobdeck-merge-{}", std::process::id()));
        fs::create_dir_all(&dist).unwrap();
        fs::write(dist.join("p.exe"), "").unwrap();
        let old = json!({"runtime": {"executables": {"windows": "p.exe", "linux": "gone"}, "executable": "p.exe"}});
        fs::write(dist.join("manifest.json"), old.to_string()).unwrap();

        let mut m = json!({"id": "p", "runtime": {"executables": {"macos": "p"}, "executable": "p"}});
        merge_other_os_executables(&mut m, &dist);
        assert_eq!(m["runtime"]["executables"], json!({"macos": "p", "windows": "p.exe"}));
        assert!(m.get("setup").is_none(), "no setup section may be added: {m}");
        let _ = fs::remove_dir_all(&dist);
    }
}
