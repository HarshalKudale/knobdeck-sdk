//! Placeholders in action text: `{selection}`, `{files}`, `{file}`, `{clipboard}`, `{app}`,
//! `{title}`, `{url}` (the browser's address), `{document}` (the open file, macOS), `{date}`,
//! `{time}`, `{date:%Y-%m-%d}`. One parser for KnobDeck's built-in actions and for plugins
//! (`ctx.expand` in the SDK).
//!
//! - Single braces, as in Raycast-style snippets. `{name}` is special only for the names above,
//!   so other text with braces (JSON, code) is left alone. `{{` and `}}` are literal braces.
//! - `{date}` / `{time}` take an optional `strftime` format: `{date:%A %d %B}`, `{time:%H:%M}`.
//! - `{input:Prompt}` asks with the OS's input box when the text is expanded (each prompt once);
//!   cancelling it fails the expansion with [`crate::input_box::CANCELLED`], so the action stops.
//! - Values are escaped for where the text goes ([`Escape`]): percent-encoded in a URL, quoted
//!   for a shell. `{selection:raw}` (or `:url`, `:shell`) overrides that per placeholder.
//! - `{files}` is the selected files: one path per line as raw text, each path quoted and
//!   space-separated for a shell (`{files:quoted}` asks for that anywhere). `{file}` is the first.
//!   The caller returns the list for [`Name::Files`] as paths joined with `\n`; `{file}` uses the
//!   same value, so the callback is never asked for [`Name::File`].
//! - The caller supplies the values, reading only the names the text uses ([`used`]).

use crate::Result;
use std::fmt::Write;

/// A placeholder name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Name {
    /// The target app's selected text.
    Selection,
    /// The files selected in the target file manager: paths joined with `\n`.
    Files,
    /// The first selected file (read through [`Name::Files`]).
    File,
    /// The clipboard's text.
    Clipboard,
    /// The target window's app name.
    App,
    /// The target window's title.
    Title,
    /// The address of the target browser's current tab.
    Url,
    /// The path of the file the target window has open (macOS).
    Document,
    /// Today's date (local), `%Y-%m-%d` unless a format is given.
    Date,
    /// The local time, `%H:%M` unless a format is given.
    Time,
    /// Asked for with the OS's input box; the argument is the prompt (`{input:Search for}`).
    Input,
}

impl Name {
    fn parse(s: &str) -> Option<Name> {
        Some(match s {
            "selection" => Name::Selection,
            "files" => Name::Files,
            "file" => Name::File,
            "clipboard" => Name::Clipboard,
            "app" => Name::App,
            "title" => Name::Title,
            "url" => Name::Url,
            "document" => Name::Document,
            "date" => Name::Date,
            "time" => Name::Time,
            "input" => Name::Input,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Name::Selection => "selection",
            Name::Files => "files",
            Name::File => "file",
            Name::Clipboard => "clipboard",
            Name::App => "app",
            Name::Title => "title",
            Name::Url => "url",
            Name::Document => "document",
            Name::Date => "date",
            Name::Time => "time",
            Name::Input => "input",
        }
    }

    /// Every name, in menu order.
    pub const ALL: [Name; 11] = [Name::Selection, Name::Files, Name::File, Name::Clipboard, Name::App, Name::Title, Name::Url, Name::Document, Name::Date, Name::Time, Name::Input];
}

/// How values are written into the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    /// As they are (a snippet, a program argument, a path).
    Raw,
    /// Percent-encoded as a URL component, except a placeholder that starts the text, which is
    /// inserted as it is (`{clipboard}` alone opens a copied URL).
    Url,
    /// Single-quoted for a POSIX shell (sh, bash, zsh, fish): `'it'\''s'`.
    Shell,
    /// An AppleScript (and JXA) string literal: `"say \"hi\""`, newlines as `\n`. `{files}` becomes a list of them:
    /// `{"/a", "/b"}`.
    AppleScript,
}

#[derive(Debug, PartialEq)]
enum Seg<'a> {
    Text(&'a str),
    Var { name: Name, arg: Option<&'a str>, at_start: bool },
}

fn segments(s: &str) -> Vec<Seg<'_>> {
    let mut out = vec![];
    let mut i = 0;
    let mut text_from = 0;
    let b = s.as_bytes();
    while i < b.len() {
        match b[i] {
            b'{' if b.get(i + 1) == Some(&b'{') => {
                out.push(Seg::Text(&s[text_from..i + 1]));
                i += 2;
                text_from = i;
            }
            b'}' if b.get(i + 1) == Some(&b'}') => {
                out.push(Seg::Text(&s[text_from..i + 1]));
                i += 2;
                text_from = i;
            }
            b'{' => {
                let Some(len) = s[i + 1..].find('}') else { break };
                let inner = &s[i + 1..i + 1 + len];
                let (name, arg) = match inner.split_once(':') {
                    Some((n, a)) => (n, Some(a)),
                    None => (inner, None),
                };
                match Name::parse(name.trim()) {
                    Some(name) => {
                        out.push(Seg::Text(&s[text_from..i]));
                        out.push(Seg::Var { name, arg, at_start: i == 0 });
                        i += len + 2;
                        text_from = i;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    out.push(Seg::Text(&s[text_from..]));
    out.retain(|seg| !matches!(seg, Seg::Text("")));
    out
}

/// The placeholder names `text` uses, each once, in order: the values to read. `{file}` is
/// reported as [`Name::Files`].
pub fn used(text: &str) -> Vec<Name> {
    let mut names = vec![];
    for seg in segments(text) {
        if let Seg::Var { name, .. } = seg {
            let name = if name == Name::File { Name::Files } else { name };
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Whether `text` has any placeholder (or `{{` / `}}`) to process.
pub fn has_any(text: &str) -> bool {
    segments(text).iter().any(|s| matches!(s, Seg::Var { .. })) || text.contains("{{") || text.contains("}}")
}

/// Expand `text`. `value` returns a placeholder's value (called once per name used; `Date`,
/// `Time` and `Input` are handled here, the callback isn't asked for them). A value error, or a
/// cancelled input box, stops the expansion.
pub fn expand(text: &str, escape: Escape, value: impl FnMut(Name) -> Result<String>) -> Result<String> {
    expand_with(text, escape, value, crate::input_box::ask, |_, v| v, false)
}

/// [`expand`], passing every final value (dates and input answers too) through `map` before it
/// is escaped and inserted: for targets with no safe inline quoting, `map` can stash the value
/// elsewhere (an environment variable) and return a reference to it, with `Escape::Raw`.
/// `{files}` is mapped one path at a time and the results joined with spaces, so each file is
/// its own argument.
pub fn expand_mapped(text: &str, escape: Escape, value: impl FnMut(Name) -> Result<String>, map: impl FnMut(Name, String) -> String) -> Result<String> {
    expand_with(text, escape, value, crate::input_box::ask, map, true)
}

fn expand_with(
    text: &str,
    escape: Escape,
    mut value: impl FnMut(Name) -> Result<String>,
    mut ask: impl FnMut(&str) -> Result<String>,
    mut map: impl FnMut(Name, String) -> String,
    mapped: bool,
) -> Result<String> {
    let mut cache: Vec<(Name, String)> = vec![];
    let mut answers: Vec<(String, String)> = vec![];
    let mut out = String::with_capacity(text.len());
    for seg in segments(text) {
        match seg {
            Seg::Text(t) => out.push_str(t),
            Seg::Var { name, arg, at_start } => {
                let (fmt, mode) = split_arg(name, arg);
                let v = match name {
                    Name::Date | Name::Time => now(name, fmt),
                    Name::Input => {
                        let prompt = arg.map(str::trim).filter(|p| !p.is_empty()).unwrap_or("Input");
                        match answers.iter().find(|(p, _)| p == prompt) {
                            Some((_, v)) => v.clone(),
                            None => {
                                let v = ask(prompt)?;
                                answers.push((prompt.to_string(), v.clone()));
                                v
                            }
                        }
                    }
                    _ => {
                        let read = if name == Name::File { Name::Files } else { name };
                        let v = match cache.iter().find(|(n, _)| *n == read) {
                            Some((_, v)) => v.clone(),
                            None => {
                                let v = value(read)?;
                                cache.push((read, v.clone()));
                                v
                            }
                        };
                        if name == Name::File {
                            v.lines().next().unwrap_or_default().to_string()
                        } else {
                            v
                        }
                    }
                };
                let esc = mode.unwrap_or(if escape == Escape::Url && at_start { Escape::Raw } else { escape });
                if name == Name::Files && esc == Escape::AppleScript && !mapped {
                    out.push('{');
                    for (i, p) in v.lines().filter(|p| !p.is_empty()).enumerate() {
                        if i > 0 {
                            out.push_str(", ");
                        }
                        write_escaped(&mut out, p, esc);
                    }
                    out.push('}');
                    continue;
                }
                if name == Name::Files && (mapped || esc == Escape::Shell) {
                    // Each path its own argument.
                    let paths = v.lines().filter(|p| !p.is_empty());
                    for (i, p) in paths.enumerate() {
                        if i > 0 {
                            out.push(' ');
                        }
                        let p = if mapped { map(name, p.to_string()) } else { p.to_string() };
                        write_escaped(&mut out, &p, if mapped { mode.unwrap_or(escape) } else { esc });
                    }
                    continue;
                }
                let v = map(name, v);
                write_escaped(&mut out, &v, esc);
            }
        }
    }
    Ok(out)
}

/// `{date:%Y}` → a format; `{selection:raw}` → an escape override.
fn split_arg(name: Name, arg: Option<&str>) -> (Option<&str>, Option<Escape>) {
    let mode = |a: &str| match a.trim() {
        "raw" => Some(Escape::Raw),
        "url" => Some(Escape::Url),
        "shell" | "quoted" => Some(Escape::Shell),
        "applescript" => Some(Escape::AppleScript),
        _ => None,
    };
    match (name, arg) {
        (_, None) | (Name::Input, _) => (None, None), // the input's argument is its prompt
        (Name::Date | Name::Time, Some(a)) => match mode(a) {
            Some(m) => (None, Some(m)),
            None => (Some(a), None),
        },
        (_, Some(a)) => (None, mode(a)),
    }
}

fn now(name: Name, fmt: Option<&str>) -> String {
    let default = if name == Name::Date { "%Y-%m-%d" } else { "%H:%M" };
    let t = chrono::Local::now();
    let mut s = String::new();
    // A bad format is an error from Display; fall back to the default instead of panicking.
    if write!(s, "{}", t.format(fmt.unwrap_or(default))).is_err() {
        s.clear();
        let _ = write!(s, "{}", t.format(default));
    }
    s
}

fn write_escaped(out: &mut String, v: &str, esc: Escape) {
    match esc {
        Escape::Raw => out.push_str(v),
        Escape::Url => {
            for b in v.bytes() {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    out.push(b as char);
                } else {
                    let _ = write!(out, "%{b:02X}");
                }
            }
        }
        Escape::AppleScript => {
            out.push('"');
            for c in v.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    // Valid in AppleScript and JavaScript (JXA) string literals alike.
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Escape::Shell => {
            out.push('\'');
            out.push_str(&v.replace('\'', r"'\''"));
            out.push('\'');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(n: Name) -> Result<String> {
        Ok(match n {
            Name::Selection => "it's a & b".into(),
            Name::Clipboard => "https://x.y/?a=1".into(),
            Name::App => "Code".into(),
            Name::Title => "main.rs".into(),
            Name::Files => "/a/it's.png\n/b/c d.jpg".into(),
            _ => unreachable!(),
        })
    }

    #[test]
    fn plain_text_and_unknown_braces_stay() {
        assert_eq!(expand(r#"{"a": 1} {name}"#, Escape::Raw, vals).unwrap(), r#"{"a": 1} {name}"#);
        assert_eq!(expand("{{selection}} }} {", Escape::Raw, vals).unwrap(), "{selection} } {");
        assert!(!has_any("no placeholders {here}"));
        assert!(has_any("a {{ b"));
    }

    #[test]
    fn raw_url_and_shell() {
        assert_eq!(expand("{app}: {selection}", Escape::Raw, vals).unwrap(), "Code: it's a & b");
        assert_eq!(expand("https://g.co/search?q={selection}", Escape::Url, vals).unwrap(), "https://g.co/search?q=it%27s%20a%20%26%20b");
        assert_eq!(expand("{clipboard}", Escape::Url, vals).unwrap(), "https://x.y/?a=1");
        assert_eq!(expand("echo {selection}", Escape::Shell, vals).unwrap(), r"echo 'it'\''s a & b'");
        assert_eq!(expand("q={selection:raw}", Escape::Url, vals).unwrap(), "q=it's a & b");
    }

    #[test]
    fn files_per_line_quoted_or_mapped() {
        assert_eq!(expand("{files}", Escape::Raw, vals).unwrap(), "/a/it's.png\n/b/c d.jpg");
        assert_eq!(expand("open {files}", Escape::Shell, vals).unwrap(), r"open '/a/it'\''s.png' '/b/c d.jpg'");
        assert_eq!(expand("{files:quoted}", Escape::Raw, vals).unwrap(), r"'/a/it'\''s.png' '/b/c d.jpg'");
        assert_eq!(expand("cat {file}", Escape::Shell, vals).unwrap(), r"cat '/a/it'\''s.png'");
        let mut n = 0;
        let out = expand_mapped("x {files} {file}", Escape::Raw, vals, |name, _| {
            n += 1;
            format!("%{}{n}%", name.as_str())
        });
        assert_eq!(out.unwrap(), "x %files1% %files2% %file3%");
        assert_eq!(used("{file} {files}"), vec![Name::Files]);
    }

    #[test]
    fn applescript_literals_and_lists() {
        assert_eq!(expand("display dialog {selection}", Escape::AppleScript, |_| Ok(r#"say "hi" \ bye"#.into())).unwrap(), r#"display dialog "say \"hi\" \\ bye""#);
        assert_eq!(expand("set f to {files}", Escape::AppleScript, vals).unwrap(), r#"set f to {"/a/it's.png", "/b/c d.jpg"}"#);
        assert_eq!(expand("{file}", Escape::AppleScript, vals).unwrap(), r#""/a/it's.png""#);
        assert_eq!(expand("{selection}", Escape::AppleScript, |_| Ok("a\nb\tc".into())).unwrap(), r#""a\nb\tc""#);
        assert_eq!(expand("{selection:applescript}", Escape::Raw, |_| Ok("x".into())).unwrap(), r#""x""#);
    }

    #[test]
    fn used_lists_each_once() {
        assert_eq!(used("{title} {selection} {title} {date:%Y}"), vec![Name::Title, Name::Selection, Name::Date]);
        assert_eq!(used("{ selection }"), vec![Name::Selection]);
    }

    #[test]
    fn values_are_read_once_and_errors_stop() {
        let mut calls = 0;
        let out = expand("{selection}{selection}", Escape::Raw, |_| {
            calls += 1;
            Ok("x".into())
        });
        assert_eq!((out.unwrap(), calls), ("xx".into(), 1));
        assert!(expand("{clipboard}", Escape::Raw, |_| Err(crate::Error::msg("off"))).is_err());
    }

    #[test]
    fn inputs_ask_once_per_prompt_and_cancel_stops() {
        let mut asked = vec![];
        let out = expand_with("git commit -m {input:Message: one line} {input:Message: one line} {input}", Escape::Shell, vals, |p| {
            asked.push(p.to_string());
            Ok("it's".into())
        }, |_, v| v, false);
        assert_eq!(out.unwrap(), r"git commit -m 'it'\''s' 'it'\''s' 'it'\''s'");
        assert_eq!(asked, vec!["Message: one line", "Input"]);
        let cancelled = expand_with("{input:x}", Escape::Raw, vals, |_| Err(crate::Error::msg(crate::input_box::CANCELLED)), |_, v| v, false);
        assert!(crate::input_box::is_cancelled(&cancelled.unwrap_err()));
    }

    #[test]
    fn mapped_values_replace_every_kind() {
        let mut seen = vec![];
        let out = expand_with("{app} {date:%Y} {input:Q}", Escape::Raw, vals, |_| Ok("a & b".into()), |n, v| {
            seen.push((n, v));
            format!("%{}%", n.as_str())
        }, true);
        assert_eq!(out.unwrap(), "%app% %date% %input%");
        assert_eq!(seen[2], (Name::Input, "a & b".to_string()));
    }

    #[test]
    fn dates_take_formats_and_survive_bad_ones() {
        let y = expand("{date:%Y}", Escape::Raw, vals).unwrap();
        assert_eq!(y.len(), 4);
        assert_eq!(expand("{date}", Escape::Raw, vals).unwrap().len(), 10);
        assert_eq!(expand("{time:%Q}", Escape::Raw, vals).unwrap().len(), 5); // bad format → %H:%M
    }
}
