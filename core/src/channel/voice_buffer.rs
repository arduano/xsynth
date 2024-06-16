use super::ChannelInitOptions;
use crate::voice::{ReleaseType, Voice};
use std::{
    collections::VecDeque,
    fmt::Debug,
    ops::{Deref, DerefMut},
};

struct GroupVoice {
    pub id: usize,
    pub voice: Box<dyn Voice>,
}

impl Deref for GroupVoice {
    type Target = Box<dyn Voice>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.voice
    }
}

impl DerefMut for GroupVoice {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Box<(dyn Voice)> {
        &mut self.voice
    }
}

impl Debug for GroupVoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("")
            .field(&self.id)
            .field(&self.voice.velocity())
            .field(&self.voice.is_killed())
            .finish()
    }
}

pub struct VoiceBuffer {
    options: ChannelInitOptions,
    id_counter: usize,
    buffer: VecDeque<GroupVoice>,
    damper_held: bool,
    held_by_damper: Vec<usize>,
}

impl VoiceBuffer {
    pub fn new(options: ChannelInitOptions) -> Self {
        VoiceBuffer {
            options,
            id_counter: 0,
            buffer: VecDeque::new(),
            damper_held: false,
            held_by_damper: Vec::new(),
        }
    }

    fn get_id(&mut self) -> usize {
        self.id_counter += 1;
        self.id_counter
    }

    /// Pops the quietest voice group. Multiple voices can be part of the same group
    /// based on their ID (e.g. a note and a hammer playing at the same time for a note on event)
    fn pop_quietest_voice_group(&mut self, ignored_id: usize) {
        if self.buffer.is_empty() {
            return;
        }

        let mut quietest = u8::MAX;
        let mut quietest_index = 0;
        let mut quietest_id = 0;
        let mut count = 0;
        for i in 0..self.buffer.len() {
            let voice = &self.buffer[i];
            if voice.id == ignored_id || voice.is_killed() {
                continue;
            }
            let vel = voice.velocity();
            if quietest_id == voice.id {
                count += 1;
            } else if vel < quietest || i == 0 {
                quietest = vel;
                quietest_index = i;
                quietest_id = voice.id;
                count = 1;
            }
        }

        if count > 0 {
            if self.options.fade_out_killing {
                for i in quietest_index..(quietest_index + count) {
                    self.kill_voice_fade_out(i);
                }
            } else {
                self.buffer.drain(quietest_index..(quietest_index + count));
            }

            if let Some(index) = self.held_by_damper.iter().position(|&x| x == quietest_id) {
                self.held_by_damper.remove(index);
            }
        }
    }

    fn kill_voice_fade_out(&mut self, index: usize) {
        self.buffer[index]
            .deref_mut()
            .signal_release(ReleaseType::Kill);
    }

    pub fn kill_all_voices(&mut self) {
        if self.options.fade_out_killing {
            for i in 0..self.buffer.len() {
                self.kill_voice_fade_out(i);
            }
            self.id_counter = 0;
        } else {
            self.buffer.clear();
        }
    }

    fn get_active_count(&mut self) -> usize {
        let mut active = 0;
        for i in 0..self.buffer.len() {
            if !self.buffer[i].deref().is_killed() {
                active += 1;
            }
        }
        active
    }

    /// Pushes a new set of voices for a single note on event. Multiple voices can be part of the same group
    /// based on their ID (e.g. a note and a hammer playing at the same time for a note on event)
    pub fn push_voices(
        &mut self,
        voices: impl Iterator<Item = Box<dyn Voice>>,
        max_voices: Option<usize>,
    ) {
        let id = self.get_id();
        for voice in voices {
            self.buffer.push_back(GroupVoice { id, voice });
        }

        if let Some(max_voices) = max_voices {
            if self.buffer.iter().filter(|v| v.id == id).count() > max_voices {
                self.pop_quietest_voice_group(id);
            } else if self.options.fade_out_killing {
                while self.get_active_count() > max_voices {
                    self.pop_quietest_voice_group(id);
                }
            } else {
                while self.buffer.len() > max_voices {
                    self.pop_quietest_voice_group(id);
                }
            }
        }
    }

    /// Releases the next voice, and all subsequent voices that have the same ID.
    pub fn release_next_voice(&mut self) -> Option<u8> {
        if !self.damper_held {
            let mut id: Option<usize> = None;
            let mut vel = None;

            // Find the first non releasing voice, get its id and release all voices with that id
            for voice in self.buffer.iter_mut() {
                if voice.is_releasing() {
                    continue;
                }

                if id.is_none() {
                    id = Some(voice.id);
                    vel = Some(voice.velocity())
                }

                if id != Some(voice.id) {
                    break;
                }

                voice.signal_release(ReleaseType::Standard);
            }

            vel
        } else {
            // Find the first non releasing voice which also isn't being held in the release buffer, and add it to the release buffer
            for voice in self.buffer.iter_mut() {
                if voice.is_releasing() {
                    continue;
                }

                if self.held_by_damper.contains(&voice.id) {
                    continue;
                }

                self.held_by_damper.push(voice.id);
                break;
            }

            None
        }
    }

    pub fn remove_ended_voices(&mut self) {
        let mut i = 0;
        while i < self.buffer.len() {
            if self.buffer[i].ended() {
                self.buffer.remove(i);
            } else {
                i += 1;
            }
        }
    }

    // pub fn iter_voices<'a>(&'a self) -> impl Iterator<Item = &Box<dyn Voice>> + 'a {
    //     self.buffer.iter().map(|group| &group.voice)
    // }

    pub fn iter_voices_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn Voice>> {
        self.buffer.iter_mut().map(|group| &mut group.voice)
    }

    pub fn has_voices(&self) -> bool {
        !self.buffer.is_empty()
    }

    pub fn voice_count(&self) -> usize {
        self.buffer.len()
    }

    pub fn set_damper(&mut self, damper: bool) {
        if self.damper_held && !damper {
            // Release all voices that are held by the damper
            for voice in self.buffer.iter_mut() {
                if self.held_by_damper.contains(&voice.id) {
                    voice.signal_release(ReleaseType::Standard);
                }
            }
            self.held_by_damper.clear();
        }
        self.damper_held = damper;
    }
}
