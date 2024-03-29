use std::{iter, ops::Deref, sync::Arc};

use crate::{
    soundfont::SoundfontBase,
    voice::{Voice, VoiceControlData},
};

use super::voice_spawner::VoiceSpawnerMatrix;

pub struct ChannelSoundfont {
    soundfonts: Vec<Arc<dyn SoundfontBase>>,
    matrix: VoiceSpawnerMatrix,
    curr_bank: Option<u8>,
    curr_preset: Option<u8>,
}

impl Deref for ChannelSoundfont {
    type Target = VoiceSpawnerMatrix;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.matrix
    }
}

impl ChannelSoundfont {
    pub fn new() -> Self {
        ChannelSoundfont {
            soundfonts: Vec::new(),
            matrix: VoiceSpawnerMatrix::new(),
            curr_bank: None,
            curr_preset: None,
        }
    }

    pub fn set_soundfonts(&mut self, soundfonts: Vec<Arc<dyn SoundfontBase>>) {
        self.soundfonts = soundfonts;
        self.curr_bank = None;
        self.curr_preset = None;
        self.rebuild_matrix(0, 0);
    }

    pub fn change_program(&mut self, bank: u8, preset: u8) {
        self.rebuild_matrix(bank, preset);
    }

    fn rebuild_matrix(&mut self, bank: u8, preset: u8) {
        if self.curr_bank == Some(bank) && self.curr_preset == Some(preset) {
            return;
        }

        for k in 0..128u8 {
            for v in 0..128u8 {
                // The fallback piano finder in case no other instrument is found
                let find_piano_attack = || {
                    self.soundfonts
                        .iter()
                        .map(|sf| sf.get_attack_voice_spawners_at(0, 0, k, v))
                        .find(|vec| !vec.is_empty())
                };

                let attack_spawners = self
                    .soundfonts
                    .iter()
                    .map(|sf| sf.get_attack_voice_spawners_at(bank, preset, k, v))
                    .chain(iter::once_with(find_piano_attack).flatten())
                    .find(|vec| !vec.is_empty())
                    .unwrap_or_default();

                // The fallback piano finder in case no other instrument is found
                let find_piano_release = || {
                    self.soundfonts
                        .iter()
                        .map(|sf| sf.get_release_voice_spawners_at(0, 0, k, v))
                        .find(|vec| !vec.is_empty())
                };

                let release_spawners = self
                    .soundfonts
                    .iter()
                    .map(|sf| sf.get_release_voice_spawners_at(bank, preset, k, v))
                    .chain(iter::once_with(find_piano_release).flatten())
                    .find(|vec| !vec.is_empty())
                    .unwrap_or_default();

                self.matrix.set_spawners_attack(k, v, attack_spawners);
                self.matrix.set_spawners_release(k, v, release_spawners);
            }
        }

        self.curr_bank = Some(bank);
        self.curr_preset = Some(preset);
    }

    pub fn spawn_voices_attack<'a>(
        &'a self,
        control: &'a VoiceControlData,
        key: u8,
        vel: u8,
    ) -> impl Iterator<Item = Box<dyn Voice>> + 'a {
        self.matrix.spawn_voices_attack(control, key, vel)
    }

    pub fn spawn_voices_release<'a>(
        &'a self,
        control: &'a VoiceControlData,
        key: u8,
        vel: u8,
    ) -> impl Iterator<Item = Box<dyn Voice>> + 'a {
        self.matrix.spawn_voices_release(control, key, vel)
    }
}
