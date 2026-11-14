//! XDG desktop entries (`applications/*.desktop`): the installed apps on Linux, and the window
//! classes a running app's windows may carry.

use std::path::{Path, PathBuf};

/// The fields of a `.desktop` file KnobDeck uses.
#[derive(Debug, Clone, Default)]
pub struct DesktopEntry {
    pub path: PathBuf,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub working_dir: Option<String>,
    pub terminal: bool,
    pub no_display: bool,
    pub startup_wm_class: Option<String>,
    pub only_show_in: Vec<String>,
    pub not_show_in: Vec<String>,
}

impl DesktopEntry {
    /// Parse the `[Desktop Entry]` group. `None` for anything that isn't a
    /// visible-or-hidden `Application` entry with an `Exec`.
    pub fn parse(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut e = DesktopEntry { path: path.to_owned(), ..Default::default() };
        let mut in_group = false;
        let mut kind = String::new();
        let mut hidden = false;
        let mut try_exec = None;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_group = line == "[Desktop Entry]";
                continue;
            }
            if !in_group || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), unescape_value(v.trim()));
            let list = |v: &str| v.split(';').filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
            match k {
                "Type" => kind = v,
                "Name" => e.name = v,
                "Exec" => e.exec = v,
                "TryExec" => try_exec = Some(v),
                "Icon" => e.icon = Some(v).filter(|s| !s.is_empty()),
                "Path" => e.working_dir = Some(v).filter(|s| !s.is_empty()),
                "Terminal" => e.terminal = v == "true",
                "NoDisplay" => e.no_display = v == "true",
                "Hidden" => hidden = v == "true",
                "StartupWMClass" => e.startup_wm_class = Some(v).filter(|s| !s.is_empty()),
                "OnlyShowIn" => e.only_show_in = list(&v),
                "NotShowIn" => e.not_show_in = list(&v),
                _ => {}
            }
        }
        if kind != "Application" || hidden || e.exec.is_empty() || e.name.is_empty() {
            return None;
        }
        if try_exec.is_some_and(|t| in_path(&t).is_none()) {
            return None;
        }
        Some(e)
    }

    /// Whether the entry belongs in this desktop's app list.
    pub fn shown_here(&self) -> bool {
        if self.no_display {
            return false;
        }
        let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .map(|s| s.to_ascii_lowercase())
            .collect();
        let has = |list: &[String]| list.iter().any(|d| desktops.contains(&d.to_ascii_lowercase()));
        (self.only_show_in.is_empty() || has(&self.only_show_in)) && !has(&self.not_show_in)
    }

    /// The command line with field codes resolved: `%f %F %u %U` take
    /// `extra` (the action's arguments), `%i %c %k` expand per the spec.
    pub fn argv(&self, extra: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        let mut used_extra = false;
        for word in split_exec(&self.exec) {
            match word.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    out.extend(extra.iter().cloned());
                    used_extra = true;
                }
                "%i" => {
                    if let Some(icon) = &self.icon {
                        out.push("--icon".into());
                        out.push(icon.clone());
                    }
                }
                _ => {
                    let w = word
                        .replace("%c", &self.name)
                        .replace("%k", &self.path.to_string_lossy())
                        .replace("%%", "\u{0}")
                        .split('%')
                        .enumerate()
                        // Drop deprecated/unknown codes (%d, %D, %n, %N, %v, %m).
                        .map(|(i, s)| if i == 0 { s.to_owned() } else { s.chars().skip(1).collect() })
                        .collect::<String>()
                        .replace('\u{0}', "%");
                    if !w.is_empty() {
                        out.push(w);
                    }
                }
            }
        }
        if !used_extra {
            out.extend(extra.iter().cloned());
        }
        out
    }

    /// Lower-cased names a running window of this app may carry as its class.
    pub fn class_candidates(&self) -> Vec<String> {
        let mut c = Vec::new();
        if let Some(w) = &self.startup_wm_class {
            c.push(w.to_ascii_lowercase());
        }
        if let Some(stem) = self.path.file_stem() {
            c.push(stem.to_string_lossy().to_ascii_lowercase());
        }
        if let Some(bin) = split_exec(&self.exec).into_iter().find(|w| !w.contains('=') && w != "env") {
            if let Some(name) = Path::new(&bin).file_name() {
                c.push(name.to_string_lossy().to_ascii_lowercase());
            }
        }
        c.push(self.name.to_ascii_lowercase());
        c.dedup();
        c
    }
}

/// Desktop Entry value escapes (`\s \n \t \r \\`).
fn unescape_value(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Split an `Exec` value into words: whitespace-separated, with
/// double-quoted words whose `\"`, `` \` ``, `\$` and `\\` are escapes.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

/// `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, the roots of `applications/` and `icons/`.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    dirs.extend(home);
    let sys = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    // Flatpak exports, in case XDG_DATA_DIRS wasn't set up for them.
    for extra in ["/var/lib/flatpak/exports/share"] {
        let p = PathBuf::from(extra);
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    if let Some(h) = std::env::var_os("HOME") {
        let p = PathBuf::from(h).join(".local/share/flatpak/exports/share");
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    dirs
}

/// Every desktop entry, with earlier data dirs shadowing later ones by desktop-file id.
pub fn desktop_entries() -> Vec<DesktopEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for root in data_dirs() {
        let apps = root.join("applications");
        let mut stack = vec![apps.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "desktop") {
                    continue;
                }
                let id = path.strip_prefix(&apps).map(|p| p.to_string_lossy().replace('/', "-")).unwrap_or_default();
                if !seen.insert(id) {
                    continue;
                }
                if let Some(e) = DesktopEntry::parse(&path) {
                    out.push(e);
                }
            }
        }
    }
    out
}

/// `program` itself when it holds a `/`, else the first match on `PATH`.
pub fn in_path(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = PathBuf::from(program);
        return p.is_file().then_some(p);
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(program)).find(|p| p.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_split_and_expand() {
        assert_eq!(split_exec(r#"env FOO=1 "/opt/My App/app" --flag %U"#), ["env", "FOO=1", "/opt/My App/app", "--flag", "%U"]);
        let e = DesktopEntry { exec: r#"code --new-window %F"#.into(), name: "Code".into(), icon: Some("vscode".into()), ..Default::default() };
        assert_eq!(e.argv(&["a.txt".into()]), ["code", "--new-window", "a.txt"]);
        let e = DesktopEntry { exec: "firefox %u %i".into(), icon: Some("firefox".into()), ..Default::default() };
        assert_eq!(e.argv(&[]), ["firefox", "--icon", "firefox"]);
        let e = DesktopEntry { exec: "app --pct=100%% --k %k".into(), ..Default::default() };
        assert_eq!(e.argv(&["x".into()]), ["app", "--pct=100%", "--k", "x"]);
    }

    #[test]
    fn desktop_values_unescape() {
        assert_eq!(unescape_value(r"a\sb\\c"), r"a b\c");
    }

    #[test]
    fn class_candidates_cover_wm_class_id_and_binary() {
        let e = DesktopEntry { path: "/usr/share/applications/org.gnome.Nautilus.desktop".into(), name: "Files".into(), exec: "/usr/bin/nautilus --new-window %U".into(), startup_wm_class: Some("Nautilus".into()), ..Default::default() };
        assert_eq!(e.class_candidates(), ["nautilus", "org.gnome.nautilus", "nautilus", "files"]);
    }
}
