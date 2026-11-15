//! `new` (create a plugin crate) and `skill` (install the AI agent skill).

use std::fs;
use std::path::{Path, PathBuf};

const CARGO_TOML: &str = include_str!("../../../templates/plugin/Cargo.toml.tmpl");
const MAIN_RS: &str = include_str!("../../../templates/plugin/main.rs.tmpl");
const README: &str = include_str!("../../../templates/plugin/readme.md.tmpl");
const GITIGNORE: &str = include_str!("../../../templates/plugin/gitignore.tmpl");

/// The agent skill, as (path inside the skill folder, contents).
pub const SKILL: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../../../skills/knobdeck-plugin/SKILL.md")),
    ("references/api.md", include_str!("../../../skills/knobdeck-plugin/references/api.md")),
    ("references/examples.md", include_str!("../../../skills/knobdeck-plugin/references/examples.md")),
];

const AGENTS_MD: &str = "# {{name}}: a KnobDeck plugin\n\n\
This folder is a KnobDeck plugin written in Rust with `knobdeck-sdk`. Before changing it, read\n\
`.claude/skills/knobdeck-plugin/SKILL.md` (plain Markdown, for any coding agent): how to choose\n\
the route to the target app (an OS API or the app's own interface, never a keyboard shortcut\n\
when one exists), the action types, and how to build and test.\n\n\
- Build: `knobdeck-plugin dev`\n\
- Test over the real protocol: `knobdeck-plugin drive [steps.json]`\n\
- Package: `knobdeck-plugin build && knobdeck-plugin package`\n";

const SDK_GIT: &str = "knobdeck-sdk = { git = \"https://github.com/Knobdeck/knobdeck-sdk.git\" }";

pub struct NewOptions {
    pub dir: PathBuf,
    pub id: Option<String>,
    pub name: Option<String>,
    pub os: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
}

/// `my_plugin`, `My Plugin` → `my-plugin`.
pub fn to_id(s: &str) -> String {
    let mut id = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_matches('-').to_string()
}

/// `my-plugin` → `My Plugin`.
pub fn to_name(id: &str) -> String {
    id.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A Rust string literal body: escape `\` and `"`.
fn rust_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// The nearest enclosing Cargo workspace root above `dir`, if any.
fn enclosing_workspace(dir: &Path) -> Option<(PathBuf, String)> {
    let start = if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().ok()?.join(dir) };
    let mut p = start.parent();
    while let Some(d) = p {
        let toml = d.join("Cargo.toml");
        if let Ok(text) = fs::read_to_string(&toml) {
            if text.contains("[workspace]") {
                return Some((d.to_path_buf(), text));
            }
        }
        p = d.parent();
    }
    None
}

fn fill(t: &str, vars: &[(&str, &str)]) -> String {
    let mut s = t.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write the skill into `dir` (the skill's own folder, e.g. `.claude/skills/knobdeck-plugin`).
pub fn install_skill(dir: &Path) -> Result<(), String> {
    for (rel, text) in SKILL {
        write(&dir.join(rel), text)?;
    }
    Ok(())
}

/// Create a plugin crate. Returns notes to print (e.g. "add it to the workspace").
pub fn new_plugin(o: &NewOptions) -> Result<Vec<String>, String> {
    if o.dir.exists() && fs::read_dir(&o.dir).map_err(|e| e.to_string())?.next().is_some() {
        return Err(format!("{} already exists and is not empty", o.dir.display()));
    }
    let folder = o.dir.file_name().and_then(|n| n.to_str()).ok_or("give the plugin a folder name")?;
    let id = to_id(o.id.as_deref().unwrap_or(folder));
    if id.is_empty() {
        return Err("the plugin id needs at least one letter or digit".into());
    }
    let name = o.name.clone().unwrap_or_else(|| to_name(&id));
    let description = o.description.clone().unwrap_or_else(|| format!("Control {name} from KnobDeck."));
    let author = o.author.clone().unwrap_or_else(|| std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "You".into()));
    let platforms = match o.os.as_deref() {
        None | Some("all") => String::new(),
        Some(os @ ("windows" | "macos" | "linux")) => format!("\n        .platforms(&[\"{os}\"])"),
        Some(other) => return Err(format!("--os must be windows, macos, linux or all, not \"{other}\"")),
    };

    let mut notes = Vec::new();
    let mut cargo = CARGO_TOML.to_string();
    let sdk_dep = match enclosing_workspace(&o.dir) {
        Some((root, text)) if text.contains("knobdeck-sdk") => {
            // A member's [profile] is ignored (with a warning) in a workspace: the root sets it.
            if let Some(i) = cargo.find("\n[profile.release]") {
                cargo.truncate(i + 1);
            }
            notes.push(format!("Add \"{folder}\" to `members` in {}", root.join("Cargo.toml").display()));
            "knobdeck-sdk.workspace = true".to_string()
        }
        Some((root, _)) => {
            notes.push(format!(
                "{} is inside the Cargo workspace {}: add \"{folder}\" to its `members` (or `exclude`) before building",
                o.dir.display(),
                root.display()
            ));
            SDK_GIT.to_string()
        }
        None => SDK_GIT.to_string(),
    };

    let vars = [
        ("id", id.as_str()),
        ("crate", id.as_str()),
        ("name", &rust_str(&name)),
        ("author", &rust_str(&author)),
        ("description", &rust_str(&description)),
        ("platforms", platforms.as_str()),
        ("sdk_dep", sdk_dep.as_str()),
    ];
    let plain = [("id", id.as_str()), ("name", name.as_str()), ("description", description.as_str())];
    write(&o.dir.join("Cargo.toml"), &fill(&cargo, &vars))?;
    write(&o.dir.join("src/main.rs"), &fill(MAIN_RS, &vars))?;
    write(&o.dir.join("readme.md"), &fill(README, &plain))?;
    write(&o.dir.join(".gitignore"), GITIGNORE)?;
    write(&o.dir.join("AGENTS.md"), &fill(AGENTS_MD, &plain))?;
    write(&o.dir.join("CLAUDE.md"), "@AGENTS.md\n")?;
    fs::create_dir_all(o.dir.join("icons")).map_err(|e| e.to_string())?;
    install_skill(&o.dir.join(".claude/skills/knobdeck-plugin"))?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names() {
        assert_eq!(to_id("My Plugin_v2!"), "my-plugin-v2");
        assert_eq!(to_id("--x--"), "x");
        assert_eq!(to_name("home-assistant"), "Home Assistant");
    }

    #[test]
    fn new_writes_a_complete_crate() {
        let root = std::env::temp_dir().join(format!("knobdeck-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("acme-player");
        let notes = new_plugin(&NewOptions {
            dir: dir.clone(),
            id: None,
            name: Some("Acme \"Player\"".into()),
            os: Some("macos".into()),
            author: Some("Tester".into()),
            description: None,
        })
        .unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        let main = fs::read_to_string(dir.join("src/main.rs")).unwrap();
        assert!(main.contains(r#"Plugin::new("acme-player", "Acme \"Player\"")"#), "{main}");
        assert!(main.contains(r#".platforms(&["macos"])"#));
        assert!(!main.contains("{{"), "unfilled placeholder: {main}");
        let cargo = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"acme-player\"") && cargo.contains("knobdeck-sdk = { git"), "{cargo}");
        for f in ["readme.md", ".gitignore", "AGENTS.md", "CLAUDE.md", ".claude/skills/knobdeck-plugin/SKILL.md", ".claude/skills/knobdeck-plugin/references/examples.md"] {
            assert!(dir.join(f).is_file(), "missing {f}");
        }
        assert!(new_plugin(&NewOptions { dir: dir.clone(), id: None, name: None, os: None, author: None, description: None }).is_err(), "must refuse a non-empty folder");
        assert!(new_plugin(&NewOptions { dir: root.join("b"), id: None, name: None, os: Some("beos".into()), author: None, description: None }).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_member_uses_the_workspace_sdk() {
        let root = std::env::temp_dir().join(format!("knobdeck-ws-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n[workspace.dependencies]\nknobdeck-sdk = { git = \"x\" }\n").unwrap();
        let notes = new_plugin(&NewOptions { dir: root.join("thing"), id: None, name: None, os: None, author: None, description: None }).unwrap();
        let cargo = fs::read_to_string(root.join("thing/Cargo.toml")).unwrap();
        assert!(cargo.contains("knobdeck-sdk.workspace = true") && !cargo.contains("[profile"), "{cargo}");
        assert_eq!(notes.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
