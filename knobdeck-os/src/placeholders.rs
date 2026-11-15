//! Placeholders in action text: `{selection}`, `{clipboard}`, `{app}`, `{title}`, `{date}`,
//! `{time}`, `{date:%Y-%m-%d}`. One parser for KnobDeck's built-in actions and for plugins
//! (`ctx.expand` in the SDK).
//!
//! - Single braces, as in Raycast-style snippets. `{name}` is special only for the names above,
//!   so other text with braces (JSON, code) is left alone. `{{` and `}}` are literal braces.
//! - `{date}` / `{time}` take an optional `strftime` format: `{date:%A %d %B}`, `{time:%H:%M}`.
//! - Values are escaped for where the text goes ([`Escape`]): percent-encoded in a URL, quoted
//!   for a shell. `{selection:raw}` (or `:url`, `:shell`) overrides that per placeholder.
//! - The caller supplies the values, reading only the names the text uses ([`used`]).

use crate::Result;
use std::fmt::Write;

/// A placeholder name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Name {
    /// The target app's selected text.
    Selection,
    /// The clipboard's text.
    Clipboard,
    /// The target window's app name.
    App,
    /// The target window's title.
    Title,
    /// Today's date (local), `%Y-%m-%d` unless a format is given.
    Date,
    /// The local time, `%H:%M` unless a format is given.
    Time,
}

impl Name {
    fn parse(s: &str) -> Option<Name> {
        Some(match s {
            "selection" => Name::Selection,
            "clipboard" => Name::Clipboard,
            "app" => Name::App,
            "title" => Name::Title,
            "date" => Name::Date,
            "time" => Name::Time,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Name::Selection => "selection",
            Name::Clipboard => "clipboard",
            Name::App => "app",
            Name::Title => "title",
            Name::Date => "date",
            Name::Time => "time",
        }
    }

    /// Every name, in menu order.
    pub const ALL: [Name; 6] = [Name::Selection, Name::Clipboard, Name::App, Name::Title, Name::Date, Name::Time];
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

/// The placeholder names `text` uses, each once, in order.
pub fn used(text: &str) -> Vec<Name> {
    let mut names = vec![];
    for seg in segments(text) {
        if let Seg::Var { name, .. } = seg {
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

/// Expand `text`. `value` returns a placeholder's value (called once per name used; `Date` and
/// `Time` are handled here, the callback isn't asked for them). A value error stops the expansion.
pub fn expand(text: &str, escape: Escape, mut value: impl FnMut(Name) -> Result<String>) -> Result<String> {
    let mut cache: Vec<(Name, String)> = vec![];
    let mut out = String::with_capacity(text.len());
    for seg in segments(text) {
        match seg {
            Seg::Text(t) => out.push_str(t),
            Seg::Var { name, arg, at_start } => {
                let (fmt, mode) = split_arg(name, arg);
                let v = match name {
                    Name::Date | Name::Time => now(name, fmt),
                    _ => match cache.iter().find(|(n, _)| *n == name) {
                        Some((_, v)) => v.clone(),
                        None => {
                            let v = value(name)?;
                            cache.push((name, v.clone()));
                            v
                        }
                    },
                };
                let esc = mode.unwrap_or(if escape == Escape::Url && at_start { Escape::Raw } else { escape });
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
        "shell" => Some(Escape::Shell),
        _ => None,
    };
    match (name, arg) {
        (_, None) => (None, None),
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
    fn dates_take_formats_and_survive_bad_ones() {
        let y = expand("{date:%Y}", Escape::Raw, vals).unwrap();
        assert_eq!(y.len(), 4);
        assert_eq!(expand("{date}", Escape::Raw, vals).unwrap().len(), 10);
        assert_eq!(expand("{time:%Q}", Escape::Raw, vals).unwrap().len(), 5); // bad format → %H:%M
    }
}
