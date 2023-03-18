use core::{channel::ChannelInitOptions, soundfont::SoundfontInitOptions};
use directories::BaseDirs;
use realtime::config::XSynthRealtimeConfig;
use serde_derive::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
struct KDMAPIConfigFile {
    sfz_path: String,
    buffer_ms: f64,
    use_threadpool: bool,
    limit_layers: bool,
    layer_count: usize,
    fade_out_kill: bool,
    linear_envelope: bool,
    use_effects: bool,
    vel_ignore_lo: u8,
    vel_ignore_hi: u8,
}

pub struct KDMAPISettings {
    pub sfz_path: String,
    pub buffer_ms: f64,
    pub use_threadpool: bool,
    pub limit_layers: bool,
    pub layer_count: usize,
    pub fade_out_kill: bool,
    pub linear_envelope: bool,
    pub use_effects: bool,
    pub vel_ignore: RangeInclusive<u8>,
}

impl Default for KDMAPISettings {
    fn default() -> Self {
        KDMAPISettings {
            sfz_path: "".to_string(),
            buffer_ms: XSynthRealtimeConfig::default().render_window_ms,
            use_threadpool: XSynthRealtimeConfig::default().use_threadpool,
            limit_layers: true,
            layer_count: 4,
            fade_out_kill: ChannelInitOptions::default().fade_out_killing,
            linear_envelope: SoundfontInitOptions::default().linear_release,
            use_effects: SoundfontInitOptions::default().use_effects,
            vel_ignore: 0..=0,
        }
    }
}

impl KDMAPISettings {
    pub fn new_or_load() -> Self {
        let config_path = Self::get_config_path();
        if !Path::new(&config_path).exists() {
            Self::load_and_save_defaults()
        } else {
            match fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str::<KDMAPIConfigFile>(&content) {
                    Ok(cfg) => Self {
                        sfz_path: cfg.sfz_path,
                        buffer_ms: cfg.buffer_ms,
                        use_threadpool: cfg.use_threadpool,
                        limit_layers: cfg.limit_layers,
                        layer_count: cfg.layer_count,
                        fade_out_kill: cfg.fade_out_kill,
                        linear_envelope: cfg.linear_envelope,
                        use_effects: cfg.use_effects,
                        vel_ignore: cfg.vel_ignore_lo..=cfg.vel_ignore_hi,
                    },
                    Err(..) => Self::load_and_save_defaults(),
                },
                Err(..) => Self::load_and_save_defaults(),
            }
        }
    }

    pub fn save_to_file(&self) {
        let config_path = Self::get_config_path();
        let cfg = KDMAPIConfigFile {
            sfz_path: self.sfz_path.clone(),
            buffer_ms: self.buffer_ms,
            use_threadpool: self.use_threadpool,
            limit_layers: self.limit_layers,
            layer_count: self.layer_count,
            fade_out_kill: self.fade_out_kill,
            linear_envelope: self.linear_envelope,
            use_effects: self.use_effects,
            vel_ignore_lo: *self.vel_ignore.start(),
            vel_ignore_hi: *self.vel_ignore.end(),
        };
        let toml: String = toml::to_string(&cfg).unwrap();
        if Path::new(&config_path).exists() {
            fs::remove_file(&config_path).expect("Error deleting old config");
        }
        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(toml.as_bytes())
            .expect("Error creating config");
    }

    fn load_and_save_defaults() -> Self {
        let cfg = Self::default();
        Self::save_to_file(&cfg);
        println!("Load Default Config!");
        cfg
    }

    fn get_config_path() -> String {
        if let Some(base_dirs) = BaseDirs::new() {
            let mut path: PathBuf = base_dirs.config_dir().to_path_buf();
            path.push("xsynth-kdmapi");
            path.push("xsynth-kdmapi-config.toml");

            if let Ok(..) = std::fs::create_dir_all(path.parent().unwrap()) {
                if let Some(path) = path.to_str() {
                    path.to_string()
                } else {
                    "xsynth-kdmapi-config.toml".to_string()
                }
            } else {
                "xsynth-kdmapi-config.toml".to_string()
            }
        } else {
            "xsynth-kdmapi-config.toml".to_string()
        }
    }
}
