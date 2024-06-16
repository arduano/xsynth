use std::{iter, ops::Deref, sync::Arc};

use crate::{
    soundfont::SoundfontBase,
    voice::{Voice, VoiceControlData},
};

use super::voice_spawner::VoiceSpawnerMatrix;

pub struct ChannelSoundfont {
    soundfonts: Vec<Arc<dyn SoundfontBase>>,
    matrix: VoiceSpawnerMatrix,
    curr_bank: u8,
    curr_preset: u8,
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
            curr_bank: 0,
            curr_preset: 0,
        }
    }

    pub fn set_soundfonts(&mut self, soundfonts: Vec<Arc<dyn SoundfontBase>>) {
        self.soundfonts = soundfonts;
        self.rebuild_matrix(self.curr_bank, self.curr_preset, true);
    }

    pub fn change_program(&mut self, bank: u8, preset: u8) {
        self.rebuild_matrix(bank, preset, false);
    }

    fn rebuild_matrix(&mut self, bank: u8, preset: u8, force_rebuild: bool) {
        if self.curr_bank == bank && self.curr_preset == preset && !force_rebuild {
            return;
        }

        // If a preset/instr. is missing from all banks it will be muted,
        // if a preset/instr. has regions in bank 0, all missing banks will be replaced by 0,
        // if a preset/instr. has regions in any bank other than 0, all missing banks will be muted.
        // For drum patches the same applies with bank and preset switched.

        for k in 0..128u8 {
            for v in 0..128u8 {
                let find_replacement_attack = || {
                    if bank == 128 {
                        self.soundfonts
                            .iter()
                            .map(|sf| sf.get_attack_voice_spawners_at(bank, 0, k, v))
                            .find(|vec| !vec.is_empty())
                    } else {
                        self.soundfonts
                            .iter()
                            .map(|sf| sf.get_attack_voice_spawners_at(0, preset, k, v))
                            .find(|vec| !vec.is_empty())
                    }
                };

                let attack_spawners = self
                    .soundfonts
                    .iter()
                    .map(|sf| sf.get_attack_voice_spawners_at(bank, preset, k, v))
                    .chain(iter::once_with(find_replacement_attack).flatten())
                    .find(|vec| !vec.is_empty())
                    .unwrap_or_default();

                let find_replacement_release = || {
                    if bank == 128 {
                        self.soundfonts
                            .iter()
                            .map(|sf| sf.get_release_voice_spawners_at(bank, 0, k, v))
                            .find(|vec| !vec.is_empty())
                    } else {
                        self.soundfonts
                            .iter()
                            .map(|sf| sf.get_release_voice_spawners_at(0, preset, k, v))
                            .find(|vec| !vec.is_empty())
                    }
                };

                let release_spawners = self
                    .soundfonts
                    .iter()
                    .map(|sf| sf.get_release_voice_spawners_at(bank, preset, k, v))
                    .chain(iter::once_with(find_replacement_release).flatten())
                    .find(|vec| !vec.is_empty())
                    .unwrap_or_default();

                self.matrix.set_spawners_attack(k, v, attack_spawners);
                self.matrix.set_spawners_release(k, v, release_spawners);
            }
        }

        self.curr_bank = bank;
        self.curr_preset = preset;
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
