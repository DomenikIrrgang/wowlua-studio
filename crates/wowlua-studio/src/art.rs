//! The installation's art kept on disk (M10, S062): an image of the
//! client's storage is decoded once, then read from Studio's cache folder
//! (`art/<installation>/<file>.rgba`), so a session draws it at once. The
//! addon's own files are read anew each time: they change while you work.

use std::path::{Path, PathBuf};

use wowlua_casc::Rgba;
use wowlua_engine::scene::{Image, ModelSource};
use wowlua_render::client::ClientAssets;
use wowlua_render::{Assets, AtlasRegion, Display};

/// [`ClientAssets`] with the installation's decoded images kept on disk.
#[derive(Clone)]
pub struct CachedAssets {
    pub inner: ClientAssets,
    /// None: nothing kept (no cache folder).
    dir: Option<PathBuf>,
}

impl CachedAssets {
    /// The assets of the installation at `install` with its client
    /// folders `folders`: kept in their own folder of the art cache.
    pub fn new(inner: ClientAssets, install: Option<&Path>, folders: &[String]) -> CachedAssets {
        use std::hash::{Hash, Hasher};
        let dir = crate::settings::art_dir().map(|root| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            install.hash(&mut h);
            folders.hash(&mut h);
            root.join(format!("{:016x}", h.finish()))
        });
        CachedAssets { inner, dir }
    }

    /// Where an image of the client's storage is kept; none for the
    /// addon's files.
    fn place(&self, image: &Image) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let name = match image {
            Image::FileId(id) => format!("{id}"),
            Image::File(path) => {
                let lower = path.replace('/', "\\").to_ascii_lowercase();
                if self
                    .inner
                    .addons
                    .iter()
                    .any(|(p, _)| lower.starts_with(p.as_str()))
                {
                    return None;
                }
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                lower.hash(&mut h);
                format!("p{:016x}", h.finish())
            }
            _ => return None,
        };
        Some(dir.join(format!("{name}.rgba")))
    }
}

/// An image as kept: its width and height (little endian), its pixels.
fn encode(rgba: &Rgba) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + rgba.pixels.len());
    out.extend_from_slice(&rgba.width.to_le_bytes());
    out.extend_from_slice(&rgba.height.to_le_bytes());
    out.extend_from_slice(&rgba.pixels);
    out
}

fn decode(bytes: &[u8]) -> Option<Rgba> {
    let width = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let height = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let pixels = bytes.get(8..)?.to_vec();
    (pixels.len() as u64 == width as u64 * height as u64 * 4).then_some(Rgba {
        width,
        height,
        pixels,
    })
}

impl Assets for CachedAssets {
    fn image(&mut self, image: &Image) -> Option<Rgba> {
        let place = self.place(image);
        if let Some(kept) = place
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| decode(&b))
        {
            return Some(kept);
        }
        let rgba = self.inner.image(image)?;
        if let Some(place) = place {
            // (written beside, then renamed: a half-written image is never
            // read)
            if let Some(dir) = place.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let staged = place.with_extension("part");
            if std::fs::write(&staged, encode(&rgba)).is_ok() {
                let _ = std::fs::rename(&staged, &place);
            }
        }
        Some(rgba)
    }

    fn font(&mut self, path: &str) -> Option<Vec<u8>> {
        self.inner.font(path)
    }

    fn atlas(&mut self, name: &str) -> Option<AtlasRegion> {
        self.inner.atlas(name)
    }

    fn file(&mut self, id: u32) -> Option<Vec<u8>> {
        self.inner.file(id)
    }

    fn model(&mut self, source: &ModelSource) -> Option<Display> {
        self.inner.model(source)
    }

    fn terrain(
        &mut self,
        map: u32,
        x: i32,
        y: i32,
    ) -> Option<std::sync::Arc<wowlua_casc::adt::Tile>> {
        self.inner.terrain(map, x, y)
    }

    fn objects(
        &mut self,
        map: u32,
        x: i32,
        y: i32,
    ) -> Option<std::sync::Arc<wowlua_casc::obj::Objects>> {
        self.inner.objects(map, x, y)
    }

    fn building(&mut self, file: u32) -> Option<std::sync::Arc<wowlua_casc::display::Building>> {
        self.inner.building(file)
    }

    fn minimap_tile(&mut self, map: u32, x: i32, y: i32) -> Option<u32> {
        self.inner.minimap_tile(map, x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_keep_and_read_back() {
        let rgba = Rgba {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        let back = decode(&encode(&rgba)).unwrap();
        assert_eq!((back.width, back.height, back.pixels), (2, 1, rgba.pixels));
        assert!(decode(&[1, 0, 0, 0, 1, 0, 0, 0, 9]).is_none(), "cut short");
    }
}
