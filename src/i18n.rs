//! Translation strings and the persisted language preference.

use std::fs;
use std::path::PathBuf;

const CONFIG_DIR: &str = "collage-editor";
const CONFIG_FILE: &str = "settings.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Swedish,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Swedish => "sv",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" | "english" => Some(Self::English),
            "sv" | "svenska" | "swedish" => Some(Self::Swedish),
            _ => None,
        }
    }

    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::English;
        };

        let Ok(contents) = fs::read_to_string(path) else {
            return Self::English;
        };

        contents
            .lines()
            .filter_map(|line| line.trim().strip_prefix("language="))
            .find_map(Self::from_code)
            .unwrap_or(Self::English)
    }

    pub fn save(self) {
        let Some(path) = config_path() else {
            return;
        };

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let _ = fs::write(path, format!("language={}\n", self.code()));
    }

    pub fn title(self) -> &'static str {
        "Collage Editor"
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::English => "Drop photos, arrange them, and export a collage.",
            Self::Swedish => "Släpp bilder, arrangera dem och exportera en kollage.",
        }
    }

    pub fn add_photos(self) -> &'static str {
        match self {
            Self::English => "Add photos",
            Self::Swedish => "Lägg till bilder",
        }
    }

    pub fn add_photos_title(self) -> &'static str {
        match self {
            Self::English => "Add photos",
            Self::Swedish => "Lägg till bilder",
        }
    }

    pub fn images_filter(self) -> &'static str {
        match self {
            Self::English => "Images",
            Self::Swedish => "Bilder",
        }
    }

    pub fn drop_here(self) -> &'static str {
        match self {
            Self::English => "Drop photos here to start",
            Self::Swedish => "Släpp bilder här för att börja",
        }
    }

    pub fn drop_hint(self) -> &'static str {
        match self {
            Self::English => "Drag in a batch, or click Add photos · JPG, PNG, WebP, TIFF and more",
            Self::Swedish => {
                "Dra in en batch, eller klicka på Lägg till bilder · JPG, PNG, WebP, TIFF med flera"
            }
        }
    }

    pub fn pattern_grid(self) -> &'static str {
        match self {
            Self::English => "Grid",
            Self::Swedish => "Rutnät",
        }
    }

    pub fn pattern_mosaic(self) -> &'static str {
        match self {
            Self::English => "Mosaic",
            Self::Swedish => "Mosaik",
        }
    }

    pub fn pattern_strip(self) -> &'static str {
        match self {
            Self::English => "Strip",
            Self::Swedish => "Rad",
        }
    }

    pub fn pattern_hero(self) -> &'static str {
        match self {
            Self::English => "Hero",
            Self::Swedish => "Hjälte",
        }
    }

    pub fn pattern_stack(self) -> &'static str {
        match self {
            Self::English => "Stack",
            Self::Swedish => "Stapel",
        }
    }

    pub fn snap(self) -> &'static str {
        match self {
            Self::English => "Snap",
            Self::Swedish => "Snäpp",
        }
    }

    pub fn clear(self) -> &'static str {
        match self {
            Self::English => "Clear canvas",
            Self::Swedish => "Rensa ytan",
        }
    }

    pub fn export_image(self) -> &'static str {
        match self {
            Self::English => "Export image",
            Self::Swedish => "Exportera bild",
        }
    }

    pub fn export_html(self) -> &'static str {
        match self {
            Self::English => "Export HTML",
            Self::Swedish => "Exportera HTML",
        }
    }

    pub fn save_image_title(self) -> &'static str {
        match self {
            Self::English => "Save collage image",
            Self::Swedish => "Spara kollagebild",
        }
    }

    pub fn save_html_title(self) -> &'static str {
        match self {
            Self::English => "Save collage as HTML",
            Self::Swedish => "Spara kollage som HTML",
        }
    }

    pub fn photo_count(self, count: usize) -> String {
        match self {
            Self::English => format!("{count} photo{}", plural_en(count)),
            Self::Swedish => format!("{count} bild{}", plural_sv(count)),
        }
    }

    pub fn loading(self, count: usize) -> String {
        match self {
            Self::English => format!("Loading {count} photo{}…", plural_en(count)),
            Self::Swedish => format!("Laddar {count} bild{}…", plural_sv(count)),
        }
    }

    pub fn exporting(self) -> &'static str {
        match self {
            Self::English => "Exporting…",
            Self::Swedish => "Exporterar…",
        }
    }

    pub fn exported(self, path: &str) -> String {
        match self {
            Self::English => format!("Saved to {path}"),
            Self::Swedish => format!("Sparat till {path}"),
        }
    }

    pub fn load_failed(self, name: &str, error: &str) -> String {
        match self {
            Self::English => format!("Could not load {name}: {error}"),
            Self::Swedish => format!("Kunde inte läsa {name}: {error}"),
        }
    }

    pub fn load_failed_count(self, count: usize) -> String {
        match self {
            Self::English => format!("Could not load {count} photo{}.", plural_en(count)),
            Self::Swedish => format!("Kunde inte läsa {count} bild{}.", plural_sv(count)),
        }
    }

    pub fn export_failed(self, error: &str) -> String {
        match self {
            Self::English => format!("Export failed: {error}"),
            Self::Swedish => format!("Exporten misslyckades: {error}"),
        }
    }

    pub fn no_images(self) -> &'static str {
        match self {
            Self::English => "Add some photos first.",
            Self::Swedish => "Lägg till några bilder först.",
        }
    }

    pub fn no_supported_images(self) -> &'static str {
        match self {
            Self::English => "No supported image files found.",
            Self::Swedish => "Inga bildfiler som stöds hittades.",
        }
    }

    pub fn already_added(self) -> &'static str {
        match self {
            Self::English => "Those photos are already in the collage.",
            Self::Swedish => "Bilderna finns redan i kollaget.",
        }
    }

    pub fn open_folder(self) -> &'static str {
        match self {
            Self::English => "Open folder",
            Self::Swedish => "Öppna mappen",
        }
    }

    pub fn html_title(self) -> &'static str {
        match self {
            Self::English => "Collage",
            Self::Swedish => "Kollage",
        }
    }

    pub fn photo_alt(self, number: usize) -> String {
        match self {
            Self::English => format!("Photo {number}"),
            Self::Swedish => format!("Bild {number}"),
        }
    }

    pub fn dismiss(self) -> &'static str {
        match self {
            Self::English => "Dismiss",
            Self::Swedish => "Stäng",
        }
    }

    pub fn english(self) -> &'static str {
        "English"
    }

    pub fn swedish(self) -> &'static str {
        "Svenska"
    }
}

fn plural_en(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn plural_sv(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "er"
    }
}

fn config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(|base| PathBuf::from(base).join(CONFIG_DIR).join(CONFIG_FILE))
    }

    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|base| {
            PathBuf::from(base)
                .join("Library")
                .join("Application Support")
                .join(CONFIG_DIR)
                .join(CONFIG_FILE)
        })
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|base| PathBuf::from(base).join(".config")))
            .map(|base| base.join(CONFIG_DIR).join(CONFIG_FILE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_codes() {
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("SV"), Some(Language::Swedish));
        assert_eq!(Language::from_code("de"), None);
    }

    #[test]
    fn formats_swedish_plurals() {
        assert_eq!(Language::Swedish.photo_count(1), "1 bild");
        assert_eq!(Language::Swedish.photo_count(3), "3 bilder");
    }
}
