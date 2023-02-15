use directories::BaseDirs;
use lazy_regex::Regex;
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
struct SFRegion {
    path: PathBuf,
    enabled: bool,
}

impl Default for SFRegion {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            enabled: false,
        }
    }
}

pub fn parse_list() -> Result<Vec<PathBuf>, ()> {
    let path = if let Some(base_dirs) = BaseDirs::new() {
        let mut path: PathBuf = base_dirs.config_dir().to_path_buf();
        path.push("Common SoundFonts");
        path.push("SoundFontList.csflist");

        match path.canonicalize() {
            Ok(..) => {}
            Err(..) => return Err(()),
        }
        path
    } else {
        return Err(());
    };

    let file = match fs::read_to_string(&path) {
        Ok(file) => file,
        Err(..) => return Err(()),
    };

    let mut input = file.trim();
    let mut vec: Vec<SFRegion> = Vec::new();

    let mut region = SFRegion::default();
    while input.len() > 0 {
        let sf_regex = Regex::new("sf\\.[a-z]*").unwrap();
        if let Some(m) = sf_regex.find_at(input, 0) {
            let start = m.start();
            let end = m.end();
            let result = input[start..end].trim();
            input = &input[end..];
            if result == "sf.start" {
                region = SFRegion::default();
            } else if result == "sf.path" {
                input = &input[3..];
                let line_end = input
                    .find('\n')
                    .or_else(|| input.find('\r'))
                    .unwrap_or(input.len());
                let result = &input[..line_end];
                input = &input[line_end..];
                region.path = PathBuf::from(result);
            } else if result == "sf.enabled" {
                input = &input[3..];
                let line_end = input
                    .find('\n')
                    .or_else(|| input.find('\r'))
                    .unwrap_or(input.len());
                let result = &input[..line_end];
                input = &input[line_end..];
                region.enabled = match result {
                    "1" => true,
                    _ => false,
                };
            } else if result == "sf.end" {
                vec.push(region.clone());
            }
        } else {
            input = "";
        }
    }

    let vec = vec
        .iter()
        .filter(|sf| {
            // Ignore non-SFZ
            let is_sfz = match sf.path.extension() {
                Some(ext) => ext.to_str().unwrap_or("") == "sfz",
                None => false,
            };
            sf.enabled && is_sfz && sf.path.canonicalize().is_ok()
        })
        .map(|sf| sf.path.clone())
        .collect();

    Ok(vec)
}
