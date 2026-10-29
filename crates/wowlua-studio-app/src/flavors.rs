//! wowlua's flavor table as Studio shows it (S064): every picker of a
//! flavor lists the table's flavors, the supported ones selectable, the
//! ones coming soon named but not picked.

use wowlua_flavors::{Flavor, table};

/// Every flavor, in the table's order (the supported first).
pub fn all() -> impl Iterator<Item = &'static Flavor> {
    table().flavors.iter()
}

/// Whether wowlua runs the flavor fully (the emulated client, the
/// debugger, Blizzard's UI); an id or an alias.
pub fn supported(id: &str) -> bool {
    table().resolve(id).is_some_and(Flavor::is_supported)
}

/// The flavor's name (`WoW Forever`), or what was written when wowlua
/// doesn't know it.
pub fn name(id: &str) -> String {
    table()
        .resolve(id)
        .map_or_else(|| id.to_owned(), |f| f.name.clone())
}

/// How a picker lists it: `camelot · WoW Forever`, or `standard · Retail ·
/// coming soon`.
pub fn label(id: &str) -> String {
    match table().resolve(id) {
        Some(f) if f.is_supported() => format!("{} · {}", f.id, f.name),
        Some(f) => format!("{} · {} · coming soon", f.id, f.name),
        None => format!("{id} · unknown to wowlua"),
    }
}

/// The highest level a character reaches on the flavor (the table's; 60
/// for one wowlua doesn't know).
pub fn max_level(id: &str) -> u32 {
    table().resolve(id).map_or(60, |f| f.max_level)
}

/// The supported flavors' ids.
pub fn supported_ids() -> Vec<String> {
    all()
        .filter(|f| f.is_supported())
        .map(|f| f.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickers_name_the_flavors_and_what_is_coming() {
        assert_eq!(label("camelot"), "camelot · WoW Forever");
        assert_eq!(label("forever"), "camelot · WoW Forever");
        assert_eq!(label("standard"), "standard · Retail · coming soon");
        assert_eq!(label("mists"), "mists · unknown to wowlua");
        assert!(supported("camelot") && !supported("vanilla"));
        assert_eq!(supported_ids(), ["camelot"]);
        assert_eq!(all().next().map(|f| f.id.as_str()), Some("camelot"));
    }
}
