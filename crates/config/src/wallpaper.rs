//! Bounded image validation shared by config reload and wallpaper rendering.
use crate::{Config, Result};
use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};

pub struct Asset {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub revision: u64,
}
pub fn load(config: &Config) -> Result<Option<Asset>> {
    if config.wallpaper_image.is_empty() {
        return Ok(None);
    }
    if !std::fs::metadata(&config.wallpaper_image)
        .map_err(|e| format!("cannot inspect wallpaper: {e}"))?
        .is_file()
    {
        return Err("wallpaper must be a regular file".into());
    }
    let file = std::fs::File::open(&config.wallpaper_image)
        .map_err(|e| format!("cannot open wallpaper: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 32 * 1024 * 1024 {
        return Err("wallpaper must be a regular file no larger than 32 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("wallpaper exceeds 32 MiB".into());
    }
    let format = image::guess_format(&bytes).map_err(|_| "wallpaper is not a supported image")?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err("wallpaper must be PNG or JPEG".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| "wallpaper cannot be decoded within image limits")?
        .into_rgba8();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(Some(Asset {
        width: decoded.width(),
        height: decoded.height(),
        rgba: decoded.into_raw(),
        revision: hasher.finish(),
    }))
}
pub fn revision(config: &Config) -> Result<Option<u64>> {
    Ok(load(config)?.map(|a| a.revision))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Source, Store};
    fn png(path: &std::path::Path, red: u8) {
        image::RgbaImage::from_pixel(2, 1, image::Rgba([red, 0, 0, 255]))
            .save(path)
            .unwrap();
    }
    #[test]
    fn relative_path_and_content_changes_are_transactional() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let image = temp.path().join("image.png");
        png(&image, 50);
        std::fs::write(&path, "wallpaper_image = 'image.png'").unwrap();
        let mut store = Store::open(Source {
            path,
            explicit: true,
        })
        .unwrap();
        assert_eq!(store.active().wallpaper_image, image.to_str().unwrap());
        assert!(!store.reload().unwrap());
        png(&image, 100);
        assert!(store.reload_with(|_| Err("disk failure".into())).is_err());
        assert!(store.reload().unwrap());
        std::fs::write(&image, b"invalid image").unwrap();
        assert!(store.reload().is_err());
        png(&image, 100);
        assert!(!store.reload().unwrap());
        std::fs::remove_file(&image).unwrap();
        assert!(store.reload().is_err());
    }
    #[test]
    fn transparent_pixels_and_unsupported_formats() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("image.png");
        image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 40]))
            .save(&path)
            .unwrap();
        let config = Config {
            wallpaper_image: path.to_str().unwrap().into(),
            ..Config::default()
        };
        let asset = load(&config).unwrap().unwrap();
        assert_eq!((asset.width, asset.height), (2, 1));
        assert_eq!(&asset.rgba[..4], &[10, 20, 30, 40]);
        std::fs::write(&path, b"GIF89a").unwrap();
        assert!(load(&config).is_err());
        assert!(load(&Config::default()).unwrap().is_none());
    }
    #[test]
    fn jpeg_and_resource_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("image.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([240, 10, 10]))
            .save(&path)
            .unwrap();
        let mut config = Config {
            wallpaper_image: path.to_str().unwrap().into(),
            ..Config::default()
        };
        let asset = load(&config).unwrap().unwrap();
        assert_eq!((asset.width, asset.height), (2, 2));
        assert!(asset.rgba[0] > 200);
        let large = temp.path().join("large.png");
        image::RgbaImage::from_pixel(8193, 1, image::Rgba([0, 0, 0, 255]))
            .save(&large)
            .unwrap();
        config.wallpaper_image = large.to_str().unwrap().into();
        assert!(load(&config).is_err());
        std::fs::File::create(&large)
            .unwrap()
            .set_len(32 * 1024 * 1024 + 1)
            .unwrap();
        assert!(load(&config).is_err());
        config.wallpaper_image = temp.path().to_str().unwrap().into();
        assert!(load(&config).is_err());
    }
}
