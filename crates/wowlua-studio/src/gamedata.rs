//! The installation's game data for the world profiles (M8, S060): art by
//! file ID, a class's icon from the class sheet, a spell's or an item's
//! icon, and the client's spells searched by name or ID. Each flavor's
//! client is opened once, on the first question.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wowlua_casc::Casc;
use wowlua_casc::spells::SpellTables;
use wowlua_casc::spelltext::Caster;
use wowlua_casc::tables::Table;
use wowlua_studio_app::system::{Art, ClassSpell, Image, SpellRow};

/// The item qualities by ID (`Enum.ItemQuality`).
const QUALITIES: &[&str] = &[
    "poor",
    "common",
    "uncommon",
    "rare",
    "epic",
    "legendary",
    "artifact",
    "heirloom",
];

/// The class sheet the character screens draw class icons from.
const CLASS_SHEET: &str = "Interface\\Glues\\CharacterCreate\\UI-CharacterCreate-Classes";

/// Each class's part of the sheet: left, right, top, bottom (Blizzard's
/// `CLASS_ICON_TCOORDS`, `Blizzard_SharedXML/SharedConstants.lua`).
const CLASS_TCOORDS: &[(&str, [f32; 4])] = &[
    ("WARRIOR", [0.0, 0.25, 0.0, 0.25]),
    ("MAGE", [0.25, 0.49609375, 0.0, 0.25]),
    ("ROGUE", [0.49609375, 0.7421875, 0.0, 0.25]),
    ("DRUID", [0.7421875, 0.98828125, 0.0, 0.25]),
    ("HUNTER", [0.0, 0.25, 0.25, 0.5]),
    ("SHAMAN", [0.25, 0.49609375, 0.25, 0.5]),
    ("PRIEST", [0.49609375, 0.7421875, 0.25, 0.5]),
    ("WARLOCK", [0.7421875, 0.98828125, 0.25, 0.5]),
    ("PALADIN", [0.0, 0.25, 0.5, 0.75]),
    ("DEATHKNIGHT", [0.25, 0.5, 0.5, 0.75]),
    ("MONK", [0.5, 0.73828125, 0.5, 0.75]),
    ("DEMONHUNTER", [0.7421875, 0.98828125, 0.5, 0.75]),
    ("EVOKER", [0.0, 0.25, 0.75, 1.0]),
];

/// A class's bit in a class mask (`ChrClasses`' ID, from 1).
fn class_bit(class: &str) -> Option<u32> {
    const IDS: &[&str] = &[
        "WARRIOR",
        "PALADIN",
        "HUNTER",
        "ROGUE",
        "PRIEST",
        "DEATHKNIGHT",
        "SHAMAN",
        "MAGE",
        "WARLOCK",
        "MONK",
        "DRUID",
        "DEMONHUNTER",
        "EVOKER",
    ];
    let i = IDS.iter().position(|c| c.eq_ignore_ascii_case(class))?;
    Some(1 << i)
}

/// The spell tables and the names searched.
struct Spells {
    names: Table,
    tables: SpellTables,
}

#[derive(Default)]
pub struct GameData {
    clients: Mutex<HashMap<String, Option<Arc<Casc>>>>,
    spells: Mutex<HashMap<String, Option<Arc<Spells>>>>,
    items: Mutex<HashMap<String, Option<Arc<Table>>>>,
    sparse: Mutex<HashMap<String, Option<Arc<Table>>>>,
    /// The creatures each flavor's client knows (wowlua T531), read once.
    creatures: Mutex<HashMap<String, Arc<Vec<wowlua_casc::display::Creature>>>>,
    /// What draws the models' previews (S082), made on first use.
    renderer: Mutex<Option<Result<wowlua_render::Renderer, String>>>,
}

/// A creature's type, as `UnitCreatureType` names it (`CreatureType`).
fn creature_type(kind: u32) -> Option<&'static str> {
    Some(match kind {
        1 => "Beast",
        2 => "Dragonkin",
        3 => "Demon",
        4 => "Elemental",
        5 => "Giant",
        6 => "Undead",
        7 => "Humanoid",
        8 => "Critter",
        9 => "Mechanical",
        10 => "Not specified",
        11 => "Totem",
        12 => "Non-combat Pet",
        13 => "Gas Cloud",
        _ => return None,
    })
}

/// A creature's classification, as `UnitClassification` answers.
fn creature_classification(c: u32) -> Option<&'static str> {
    Some(match c {
        0 => "normal",
        1 => "elite",
        2 => "rareelite",
        3 => "worldboss",
        4 => "rare",
        5 => "trivial",
        6 => "minus",
        _ => return None,
    })
}

impl GameData {
    /// The flavor's client, opened once.
    fn client(&self, flavor: &str) -> Option<Arc<Casc>> {
        let mut clients = self.clients.lock().ok()?;
        clients
            .entry(flavor.to_string())
            .or_insert_with(|| {
                let folders: Vec<String> = wowlua_flavors::table()
                    .clients_of(flavor)
                    .flat_map(|c| c.folders.iter().cloned())
                    .collect();
                let install = wowlua_driver::install::find_wow(None).ok()?;
                wowlua_render::client::open_client(&install, &folders).ok()
            })
            .clone()
    }

    fn spell_tables(&self, flavor: &str) -> Option<Arc<Spells>> {
        let casc = self.client(flavor)?;
        let mut spells = self.spells.lock().ok()?;
        spells
            .entry(flavor.to_string())
            .or_insert_with(|| {
                Some(Arc::new(Spells {
                    names: Table::open(&casc, "SpellName")?,
                    tables: SpellTables::open(&casc)?,
                }))
            })
            .clone()
    }

    fn item_table(&self, flavor: &str) -> Option<Arc<Table>> {
        let casc = self.client(flavor)?;
        let mut items = self.items.lock().ok()?;
        items
            .entry(flavor.to_string())
            .or_insert_with(|| Table::open(&casc, "Item").map(Arc::new))
            .clone()
    }

    /// The items' names, qualities and levels (`ItemSparse`), opened once.
    fn item_sparse(&self, flavor: &str) -> Option<Arc<Table>> {
        let casc = self.client(flavor)?;
        let mut sparse = self.sparse.lock().ok()?;
        sparse
            .entry(flavor.to_string())
            .or_insert_with(|| Table::open(&casc, "ItemSparse").map(Arc::new))
            .clone()
    }

    /// The client's items whose name holds `query` (or whose ID it is),
    /// the first 40, each with its quality and item level as its text.
    pub fn search_items(&self, flavor: &str, query: &str) -> Vec<SpellRow> {
        let q = query.trim();
        let id = q.parse::<u32>().ok();
        if q.chars().count() < 2 && id.is_none() {
            return Vec::new();
        }
        let Some(sparse) = self.item_sparse(flavor) else {
            return Vec::new();
        };
        let items = self.item_table(flavor);
        let lower = q.to_lowercase();
        let mut out = Vec::new();
        for row in sparse.rows() {
            let name = row.string("Display_lang").unwrap_or_default();
            if id != Some(row.id()) && !name.to_lowercase().contains(&lower) {
                continue;
            }
            let quality = QUALITIES
                .get(row.int("OverallQualityID") as usize)
                .copied()
                .unwrap_or("");
            out.push(SpellRow {
                id: row.id(),
                name,
                icon: items
                    .as_ref()
                    .and_then(|t| t.row(row.id()))
                    .map(|r| r.int("IconFileDataID"))
                    .filter(|i| *i != 0),
                text: format!("{quality} · item level {}", row.int("ItemLevel")),
            });
            if out.len() >= 40 {
                break;
            }
        }
        out
    }

    /// The creatures the flavor's client knows whose name holds the query
    /// (or of its ID), as rows: the display as the icon, the type and
    /// classification as the text (S082).
    pub fn search_creatures(&self, flavor: &str, query: &str) -> Vec<SpellRow> {
        let q = query.trim();
        let id = q.parse::<u32>().ok();
        if q.chars().count() < 2 && id.is_none() {
            return Vec::new();
        }
        let Some(casc) = self.client(flavor) else {
            return Vec::new();
        };
        let all = {
            let Ok(mut known) = self.creatures.lock() else {
                return Vec::new();
            };
            known
                .entry(flavor.to_string())
                .or_insert_with(|| {
                    let mut lib = wowlua_casc::display::Library::default();
                    Arc::new(lib.creatures(&casc).to_vec())
                })
                .clone()
        };
        let lower = q.to_lowercase();
        all.iter()
            .filter(|c| id == Some(c.id) || c.name.to_lowercase().contains(&lower))
            .take(40)
            .map(|c| SpellRow {
                id: c.id,
                name: c.name.clone(),
                icon: c.display,
                text: [
                    c.kind.and_then(creature_type).unwrap_or("—"),
                    c.classification
                        .and_then(creature_classification)
                        .unwrap_or("—"),
                    c.family.as_deref().unwrap_or("—"),
                ]
                .join(" · "),
            })
            .collect()
    }

    /// A creature display's model as a model frame draws it, 160 pixels a
    /// side, on black (S082).
    fn model_preview(&self, casc: &Arc<Casc>, display: u32) -> Option<Image> {
        use wowlua_engine::scene;
        let item = scene::Model {
            widget: 0,
            rect: scene::Rect {
                x: 0.0,
                y: 0.0,
                w: 160.0,
                h: 160.0,
            },
            clip: None,
            alpha: 1.0,
            actors: vec![scene::Actor {
                source: scene::ModelSource::Display(display),
                position: [0.0; 3],
                yaw: 0.6,
                pitch: 0.0,
                roll: 0.0,
                fixed_axes: false,
                scale: 1.0,
                alpha: 1.0,
                animation: 0,
                variation: 0,
            }],
            camera: scene::ModelCamera::Frame {
                portrait_zoom: 0.0,
                distance_scale: 1.0,
                camera: None,
            },
            light: scene::Light {
                enabled: true,
                direction: [-1.0, 0.3, -0.6],
                ambient: [0.55; 3],
                diffuse: [0.75; 3],
            },
            time: 0.0,
        };
        let shown = scene::Scene {
            width: 160,
            height: 160,
            items: vec![scene::Item::Model(item)],
            world: None,
        };
        let mut assets = wowlua_render::client::ClientAssets::new(Some(casc.clone()), &[]);
        let mut renderer = self.renderer.lock().ok()?;
        let r = renderer
            .get_or_insert_with(wowlua_render::Renderer::new_blocking)
            .as_mut()
            .ok()?;
        let img = r.render_blocking(&shown, 1.0, None, &mut assets).ok()?;
        Some(Image {
            width: img.width,
            height: img.height,
            rgba: img.pixels,
        })
    }

    /// The art, as an image (a class's icon cropped from the sheet).
    pub fn art(&self, flavor: &str, art: &Art) -> Option<Image> {
        let casc = self.client(flavor)?;
        if let Art::Model(display) = art {
            return self.model_preview(&casc, *display);
        }
        let (bytes, crop) = match art {
            Art::File(id) => (casc.file_by_id(*id)?, None),
            Art::Class(class) => {
                let coords = CLASS_TCOORDS.iter().find(|(c, _)| c == class)?.1;
                (casc.file_by_path(CLASS_SHEET)?, Some(coords))
            }
            Art::Spell(id) => {
                let icon = self.spell_tables(flavor)?.tables.info(*id)?.icon?;
                (casc.file_by_id(icon)?, None)
            }
            Art::Item(id) => {
                let icon = self.item_table(flavor)?.row(*id)?.int("IconFileDataID");
                (casc.file_by_id(icon)?, None)
            }
            Art::Model(_) => return None,
        };
        let rgba = wowlua_casc::decode_image(&bytes)?;
        Some(cropped(rgba.width, rgba.height, &rgba.pixels, crop))
    }

    /// A class's spells (`WARRIOR`): those of its skill lines
    /// (`SkillLine`'s class category, the abilities whose class mask has
    /// the class), with their level and rank, by skill line, then level.
    pub fn class_spells(&self, flavor: &str, class: &str) -> Vec<ClassSpell> {
        let Some(bit) = class_bit(class) else {
            return Vec::new();
        };
        let Some(casc) = self.client(flavor) else {
            return Vec::new();
        };
        let Some(spells) = self.spell_tables(flavor) else {
            return Vec::new();
        };
        let (Some(lines), Some(abilities), Some(levels)) = (
            Table::open(&casc, "SkillLine"),
            Table::open(&casc, "SkillLineAbility"),
            Table::open(&casc, "SpellLevels"),
        ) else {
            return Vec::new();
        };
        let class_lines: HashMap<u32, String> = lines
            .rows()
            .filter(|r| r.int("CategoryID") == 7)
            .map(|r| (r.id(), r.string("DisplayName_lang").unwrap_or_default()))
            .collect();
        let mut out: Vec<ClassSpell> = Vec::new();
        for a in abilities.rows() {
            let Some(line) = class_lines.get(&a.int("SkillLine")) else {
                continue;
            };
            // A spell the class learns: from a trainer (0) or by reaching a
            // level (2); not one acquired otherwise (3: runes, items),
            // checked against SuuUI's trainer probes (S060).
            if a.int("ClassMask") & bit == 0 || !matches!(a.int("AcquireMethod"), 0 | 2) {
                continue;
            }
            let id = a.int("Spell");
            if out.iter().any(|s| s.id == id) {
                continue;
            }
            let Some(info) = spells.tables.info(id) else {
                continue;
            };
            let level = levels
                .rows_where("SpellID", id)
                .first()
                .map_or(0, |r| r.int("BaseLevel"));
            // Without a level: an engraving's or another season's spell.
            if level == 0 {
                continue;
            }
            out.push(ClassSpell {
                id,
                name: info.name,
                rank: info.subtext,
                level,
                line: line.clone(),
                icon: info.icon,
            });
        }
        out.sort_by(|a, b| {
            (&a.line, a.level, &a.name, a.id).cmp(&(&b.line, b.level, &b.name, b.id))
        });
        out
    }

    /// The client's spells whose name holds `query` (or whose ID it is),
    /// the first 40.
    pub fn search_spells(&self, flavor: &str, query: &str) -> Vec<SpellRow> {
        let q = query.trim();
        let id = q.parse::<u32>().ok();
        if q.chars().count() < 2 && id.is_none() {
            return Vec::new();
        }
        let Some(spells) = self.spell_tables(flavor) else {
            return Vec::new();
        };
        let lower = q.to_lowercase();
        let caster = Caster {
            level: 60,
            ..Caster::default()
        };
        let mut out = Vec::new();
        for row in spells.names.rows() {
            let hit = id == Some(row.id())
                || row
                    .string("Name_lang")
                    .is_some_and(|n| n.to_lowercase().contains(&lower));
            if !hit {
                continue;
            }
            let Some(info) = spells.tables.info(row.id()) else {
                continue;
            };
            out.push(SpellRow {
                id: row.id(),
                name: info.name.clone(),
                icon: info.icon,
                text: spells
                    .tables
                    .describe(row.id(), &caster)
                    .unwrap_or_else(|| info.subtext.clone()),
            });
            if out.len() >= 40 {
                break;
            }
        }
        out
    }
}

/// `pixels` (`width` × `height` RGBA), cropped to `coords` (left, right,
/// top, bottom from 0 to 1).
fn cropped(width: u32, height: u32, pixels: &[u8], coords: Option<[f32; 4]>) -> Image {
    let Some([l, r, t, b]) = coords else {
        return Image {
            width,
            height,
            rgba: pixels.to_vec(),
        };
    };
    let x0 = (l * width as f32).round() as u32;
    let x1 = ((r * width as f32).round() as u32).clamp(x0 + 1, width);
    let y0 = (t * height as f32).round() as u32;
    let y1 = ((b * height as f32).round() as u32).clamp(y0 + 1, height);
    let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
    for y in y0..y1 {
        let start = ((y * width + x0) * 4) as usize;
        let end = ((y * width + x1) * 4) as usize;
        rgba.extend_from_slice(&pixels[start..end]);
    }
    Image {
        width: x1 - x0,
        height: y1 - y0,
        rgba,
    }
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn a_class_s_part_of_the_sheet() {
        // A 4 × 4 sheet: the top left quarter is WARRIOR's.
        let pixels: Vec<u8> = (0..16u8).flat_map(|i| [i, 0, 0, 255]).collect();
        let coords = CLASS_TCOORDS
            .iter()
            .find(|(c, _)| *c == "WARRIOR")
            .unwrap()
            .1;
        let image = cropped(4, 4, &pixels, Some(coords));
        assert_eq!((image.width, image.height), (1, 1));
        assert_eq!(image.rgba, [0, 0, 0, 255]);
        let whole = cropped(4, 4, &pixels, None);
        assert_eq!(whole.rgba.len(), 64);
    }

    /// The client's items by name or ID (skipped without a camelot client).
    #[test]
    fn items_by_name_or_id_from_the_client() {
        let data = GameData::default();
        let found = data.search_items("camelot", "thunderfury");
        if data.item_sparse("camelot").is_none() {
            eprintln!("no camelot client: skipped");
            return;
        }
        let blade = found.iter().find(|i| i.id == 19019).expect("Thunderfury");
        assert!(blade.name.starts_with("Thunderfury"), "{}", blade.name);
        assert!(
            blade.text.starts_with("legendary · item level"),
            "{}",
            blade.text
        );
        assert!(blade.icon.is_some());
        let by_id = data.search_items("camelot", "19019");
        assert_eq!(by_id.first().map(|i| i.id), Some(19019));
        assert!(data.search_items("camelot", "t").is_empty());
    }

    /// The client's warrior spells (skipped without a camelot client).
    #[test]
    fn a_class_s_spells_from_the_client() {
        let data = GameData::default();
        let spells = data.class_spells("camelot", "WARRIOR");
        if spells.is_empty() {
            eprintln!("no camelot client: skipped");
            return;
        }
        let heroic: Vec<(u32, u32)> = spells
            .iter()
            .filter(|s| s.name == "Heroic Strike")
            .map(|s| (s.id, s.level))
            .collect();
        assert_eq!(heroic[0], (78, 1), "{heroic:?}");
        assert!(spells.iter().all(|s| s.line != "Pet - Imp"));
        // Learned otherwise (a rune), and engravings: not the class's.
        assert!(
            spells
                .iter()
                .all(|s| s.name != "Meathook" && !s.name.starts_with("Engrave"))
        );
        assert!(
            spells
                .iter()
                .any(|s| s.name == "Defensive Stance" && s.level == 10)
        );
        assert!(class_bit("warrior") == Some(1) && class_bit("DRUID") == Some(1 << 10));
    }
}
