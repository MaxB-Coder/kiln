//! Writes one setting into kiln.toml for the Settings pop-up, leaving the rest of the file as
//! the user wrote it: comments, order, spacing and other keys stay byte for byte (toml_edit).
//! A file Kiln cannot read is never written over; the pop-up shows its error instead.

use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike, Value};

use crate::config::{self, Config};

/// Why a setting was not written.
#[derive(Debug, PartialEq, Eq)]
pub enum WriteError {
    /// The file has an error (TOML, or a value Kiln cannot use); it is left alone.
    Broken(String),
    Io(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::Broken(message) | WriteError::Io(message) => f.write_str(message),
        }
    }
}

/// Whether Kiln can read this text as its config; the error's first lines when not.
pub fn check(text: &str) -> Result<(), String> {
    let short = |message: String| message.lines().take(4).collect::<Vec<_>>().join("\n");
    text.parse::<DocumentMut>()
        .map_err(|err| short(err.to_string()))?;
    config::parse(text).map_err(|err| short(err.to_string()))?;
    Ok(())
}

/// One change: the key's path (`["window", "sidebar"]`) and its new value, or `None` to
/// remove the key.
pub type Edit<'a> = (&'a [&'a str], Option<Value>);

/// `text` with the setting at `path` set to `value`. An existing value keeps its spacing and
/// trailing comment; a missing key is added to its table, and a missing table at the end of
/// the file.
pub fn with_setting(text: &str, path: &[&str], value: Value) -> Result<String, WriteError> {
    with_edits(text, &[(path, Some(value))])
}

/// `text` with several changes at once (see `with_setting`); removing a key that is not there
/// does nothing.
pub fn with_edits(text: &str, edits: &[Edit]) -> Result<String, WriteError> {
    check(text).map_err(WriteError::Broken)?;
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|err: toml_edit::TomlError| WriteError::Broken(err.to_string()))?;
    for (path, value) in edits {
        let Some((key, tables)) = path.split_last() else {
            return Err(WriteError::Broken("no key".into()));
        };
        let mut table: &mut dyn TableLike = doc.as_table_mut();
        for name in tables {
            if value.is_none() && !table.contains_key(name) {
                break;
            }
            let item = table.entry(name).or_insert_with(toml_edit::table);
            table = item
                .as_table_like_mut()
                .ok_or_else(|| WriteError::Broken(format!("{name} is not a table")))?;
        }
        match (table.get_mut(key).and_then(Item::as_value_mut), value) {
            (Some(old), Some(value)) => {
                let decor = old.decor().clone();
                *old = value.clone();
                *old.decor_mut() = decor;
            }
            (None, Some(value)) => {
                table.insert(key, Item::Value(value.clone()));
            }
            (_, None) => {
                table.remove(key);
            }
        }
    }
    Ok(doc.to_string())
}

/// Writes the setting into the config file at `path` (created from the template when missing)
/// and returns the config it now holds. Atomic: a temporary file next to it is renamed over it.
pub fn write_setting(path: &Path, key: &[&str], value: Value) -> Result<Config, WriteError> {
    write_edits(path, &[(key, Some(value))])
}

/// Like `write_setting`, with several changes in one write.
pub fn write_edits(path: &Path, edits: &[Edit]) -> Result<Config, WriteError> {
    // Through a symlink, write the file it points to (dotfile managers link it).
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = match std::fs::read_to_string(&target) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => config::TEMPLATE.to_string(),
        Err(err) => return Err(WriteError::Io(err.to_string())),
    };
    let new = with_edits(&text, edits)?;
    let config = config::parse(&new).map_err(|err| WriteError::Broken(err.to_string()))?;
    write_atomically(&target, &new).map_err(|err| WriteError::Io(err.to_string()))?;
    Ok(config)
}

fn write_atomically(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temp = PathBuf::from(path);
    temp.set_extension("toml.kiln-tmp");
    std::fs::write(&temp, text)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "# my settings\n\
        font_size   = 13    # big enough\n\
        \n\
        # keep the shell quiet\n\
        [shell]\n\
        prompt = \"shell\"\n\
        program = \"/bin/zsh\"\n\
        \n\
        [window]\n\
        sidebar = true\n\
        columns = 100 # wide\n";

    #[test]
    fn only_the_changed_value_changes() {
        let new = with_setting(USER, &["font_size"], 15.into()).unwrap();
        assert_eq!(new, USER.replace("= 13    #", "= 15    #"));
        let new = with_setting(USER, &["window", "sidebar"], false.into()).unwrap();
        assert_eq!(new, USER.replace("sidebar = true", "sidebar = false"));
        let new = with_setting(USER, &["shell", "prompt"], "kiln".into()).unwrap();
        assert_eq!(new, USER.replace("prompt = \"shell\"", "prompt = \"kiln\""));
    }

    #[test]
    fn missing_keys_and_tables_are_added() {
        // A top-level key goes before the first table.
        let new = with_setting(USER, &["icon"], "black".into()).unwrap();
        assert!(
            new.starts_with("# my settings\nfont_size   = 13    # big enough\nicon = \"black\"\n")
        );
        assert_eq!(config::parse(&new).unwrap().icon, config::AppIcon::Black);
        // A missing table goes at the end; nothing before it moves.
        let new = with_setting(USER, &["updates", "check"], false.into()).unwrap();
        assert!(new.starts_with(USER));
        assert!(new.ends_with("[updates]\ncheck = false\n"), "{new}");
        // Inline tables work too.
        let new = with_setting(
            "window = { sidebar = true }\n",
            &["window", "restore"],
            false.into(),
        )
        .unwrap();
        assert!(!config::parse(&new).unwrap().window.restore);
    }

    #[test]
    fn theme_keys_round_trip() {
        let text =
            "[theme]\nname = \"nord\" # cool\nfg = \"#010203\"\n\n[window]\nsidebar = true\n";
        // Only the name changes; the rest of the table stays as it was.
        let new = with_setting(text, &["theme", "name"], "kanagawa".into()).unwrap();
        assert_eq!(new, text.replace("\"nord\"", "\"kanagawa\""));
        // Removing keys removes exactly those.
        let cleared =
            with_edits(&new, &[(&["theme", "fg"], None), (&["theme", "bg"], None)]).unwrap();
        assert_eq!(cleared, new.replace("fg = \"#010203\"\n", ""));
        // The ANSI colours as an array.
        let ansi: toml_edit::Array = (0..16).map(|i| format!("#0000{i:02x}")).collect();
        let with_ansi = with_setting(text, &["theme", "ansi"], ansi.into()).unwrap();
        let config = config::parse(&with_ansi).unwrap();
        assert_eq!(config.theme.colours.ansi.unwrap()[15], "#00000f");
        assert!(with_ansi.ends_with("[window]\nsidebar = true\n"));
    }

    #[test]
    fn a_broken_file_is_not_written_over() {
        let dir = std::env::temp_dir().join(format!("kiln-config-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kiln.toml");
        for broken in ["font_size = = 3\n", "font_size = \"big\"\n"] {
            std::fs::write(&path, broken).unwrap();
            let result = write_setting(&path, &["cursor_blink"], false.into());
            assert!(
                matches!(result, Err(WriteError::Broken(_))),
                "{broken}: {result:?}"
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
            assert!(check(broken).is_err());
        }
        // A missing file starts from the template.
        std::fs::remove_file(&path).unwrap();
        let config = write_setting(&path, &["icon"], "white".into()).unwrap();
        assert_eq!(config.icon, config::AppIcon::White);
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            config::TEMPLATE.replace("icon = \"terracotta\"", "icon = \"white\"")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
