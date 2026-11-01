//! The binding actions a flavor's client has (S065): Blizzard's UI's
//! `<Binding>`s, from the flavor's bindings file (`Bindings_Camelot.xml`, …)
//! and each Blizzard addon's `Bindings.xml`, in the UI code wowlua keeps.

use std::path::Path;

/// The flavor's binding actions, by name, with their category
/// (`MOVEFORWARD`, `movement`); none while its UI code isn't fetched.
pub fn actions(flavor: &str) -> Vec<(String, String)> {
    let Some(source) = wowlua_test::cached_ui_source(flavor) else {
        return Vec::new();
    };
    // (the classic clients share one UI code: each its own file)
    let own = match flavor {
        "camelot" => "Bindings_Camelot.xml",
        "standard" => "Bindings_Standard.xml",
        "vanilla" => "Bindings_Vanilla.xml",
        "tbc" => "Bindings_TBC.xml",
        "mists" => "Bindings_Mists.xml",
        _ => return Vec::new(),
    };
    let mut out: Vec<(String, String)> = Vec::new();
    let addons = source.interface.join("AddOns");
    let Ok(dirs) = std::fs::read_dir(&addons) else {
        return out;
    };
    let mut dirs: Vec<_> = dirs.flatten().map(|d| d.path()).collect();
    dirs.sort();
    for dir in dirs {
        for file in [dir.join(own), dir.join("Bindings.xml")] {
            read(&file, &mut out);
        }
    }
    out
}

/// The `<Binding name=… category=…>`s of a file, each once.
fn read(file: &Path, out: &mut Vec<(String, String)>) {
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    for tag in text.split("<Binding ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let attr = |name: &str| {
            let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
            let end = tag[at..].find('"')?;
            Some(tag[at..at + end].to_owned())
        };
        let Some(name) = attr("name") else {
            continue;
        };
        if out.iter().any(|(n, _)| *n == name) {
            continue;
        }
        let category = attr("category")
            .map(|c| {
                c.trim_start_matches("BINDING_HEADER_")
                    .to_ascii_lowercase()
                    .replace('_', " ")
            })
            .unwrap_or_default();
        out.push((name, category));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_are_read_once_with_their_category() {
        let dir = tempfile::tempdir().expect("a folder");
        let file = dir.path().join("Bindings.xml");
        std::fs::write(
            &file,
            r#"<Bindings><Binding name="MOVEFORWARD" runOnUp="true" category="BINDING_HEADER_MOVEMENT">
            MoveForwardStart();</Binding><Binding name="TOGGLEMUSIC">Sound_ToggleMusic();</Binding>
            <Binding name="MOVEFORWARD">x</Binding></Bindings>"#,
        )
        .expect("written");
        let mut out = Vec::new();
        read(&file, &mut out);
        assert_eq!(
            out,
            [
                ("MOVEFORWARD".to_owned(), "movement".to_owned()),
                ("TOGGLEMUSIC".to_owned(), String::new()),
            ]
        );
    }
}
