use std::{
    collections::VecDeque,
    ops::RangeInclusive,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;

use xsynth_core::channel::{ChannelAudioEvent, ChannelConfigEvent, ChannelEvent, ControlEvent};

use crate::{util::ReadWriteAtomicU64, SynthEvent};

static NPS_WINDOW_MILLISECONDS: u64 = 20;

struct NpsWindow {
    time: u64,
    notes: u64,
}

/// A struct for tracking the estimated NPS, as fast as possible with the focus on speed
/// rather than precision. Used for NPS limiting on extremely spammy midis.
struct RoughNpsTracker {
    rough_time: Arc<ReadWriteAtomicU64>,
    last_time: u64,
    windows: VecDeque<NpsWindow>,
    total_window_sum: u64,
    current_window_sum: u64,
    stop: Arc<RwLock<bool>>,
    join_handle: Option<JoinHandle<()>>,
}

impl RoughNpsTracker {
    pub fn new() -> RoughNpsTracker {
        let rough_time = Arc::new(ReadWriteAtomicU64::new(0));
        let stop = Arc::new(RwLock::new(false));

        let join_handle = {
            let rough_time = rough_time.clone();
            let stop = stop.clone();
            thread::Builder::new()
                .name("xsynth_nps_tracker".to_string())
                .spawn(move || {
                    let mut last_time = 0;
                    let mut now = Instant::now();
                    while !*stop.read().unwrap() {
                        thread::sleep(Duration::from_millis(NPS_WINDOW_MILLISECONDS));
                        let diff = now.elapsed();
                        last_time += diff.as_millis() as u64;
                        rough_time.write(last_time);
                        now = Instant::now();
                    }
                })
                .unwrap()
        };

        RoughNpsTracker {
            rough_time,
            last_time: 0,
            windows: VecDeque::new(),
            total_window_sum: 0,
            current_window_sum: 0,
            stop,
            join_handle: Some(join_handle),
        }
    }

    pub fn calculate_nps(&mut self) -> u64 {
        self.check_time();

        loop {
            let cutoff = self.last_time - 1000;
            if let Some(window) = self.windows.front() {
                if window.time < cutoff {
                    self.total_window_sum -= window.notes;
                    self.windows.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let short_nps = self.current_window_sum * (1000 / NPS_WINDOW_MILLISECONDS) * 4 / 3;
        let long_nps = self.total_window_sum;

        short_nps.max(long_nps)
    }

    fn check_time(&mut self) {
        let time = self.rough_time.read();
        if time > self.last_time {
            self.windows.push_back(NpsWindow {
                time: self.last_time,
                notes: self.current_window_sum,
            });
            self.current_window_sum = 0;
            self.last_time = time;
        }
    }

    pub fn add_note(&mut self) {
        self.current_window_sum += 1;
        self.total_window_sum += 1;
    }
}

impl Drop for RoughNpsTracker {
    fn drop(&mut self) {
        *self.stop.write().unwrap() = true;
        self.join_handle.take().unwrap().join().unwrap();
    }
}

fn should_send_for_vel_and_nps(vel: u8, nps: u64, max: u64) -> bool {
    (vel as u64) * max / 127 > nps
}

struct EventSender {
    sender: Sender<ChannelEvent>,
    nps: RoughNpsTracker,
    max_nps: Arc<ReadWriteAtomicU64>,
    skipped_notes: [u64; 128],
    ignore_range: RangeInclusive<u8>,
}

impl EventSender {
    pub fn new(
        max_nps: Arc<ReadWriteAtomicU64>,
        sender: Sender<ChannelEvent>,
        ignore_range: RangeInclusive<u8>,
    ) -> Self {
        EventSender {
            sender,
            nps: RoughNpsTracker::new(),
            max_nps,
            skipped_notes: [0; 128],
            ignore_range,
        }
    }

    pub fn send_audio(&mut self, event: ChannelAudioEvent) {
        match &event {
            ChannelAudioEvent::NoteOn { vel, key } => {
                if *key > 127 {
                    return;
                }

                let in_ignore_range = self.ignore_range.contains(vel);

                let nps = self.nps.calculate_nps();
                if should_send_for_vel_and_nps(*vel, nps, self.max_nps.read()) && !in_ignore_range {
                    self.sender.send(ChannelEvent::Audio(event)).ok();
                    self.nps.add_note();
                } else {
                    self.skipped_notes[*key as usize] += 1;
                }
            }
            ChannelAudioEvent::NoteOff { key } => {
                if *key > 127 {
                    return;
                }

                if self.skipped_notes[*key as usize] > 0 {
                    self.skipped_notes[*key as usize] -= 1;
                } else {
                    self.sender.send(ChannelEvent::Audio(event)).ok();
                }
            }
            _ => {
                self.sender.send(ChannelEvent::Audio(event)).ok();
            }
        }
    }

    pub fn send_config(&mut self, event: ChannelConfigEvent) {
        self.sender.send(ChannelEvent::Config(event)).ok();
    }

    // pub fn send(&mut self, event: ChannelEvent) {
    //     match event {
    //         ChannelEvent::Audio(event) => {
    //             self.send_audio(event);
    //         }
    //         ChannelEvent::Config(event) => {
    //             self.send_config(event);
    //         }
    //     }
    // }
}

impl Clone for EventSender {
    fn clone(&self) -> Self {
        EventSender {
            sender: self.sender.clone(),
            max_nps: self.max_nps.clone(),

            // Rough nps tracker is only used for very extreme spam situations,
            // so creating a new one when cloning shouldn't be an issue
            nps: RoughNpsTracker::new(),

            // Skipped notes is related to nps limiter, therefore it's also not cloned
            skipped_notes: [0; 128],

            ignore_range: self.ignore_range.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RealtimeEventSender {
    senders: Vec<EventSender>,
}

impl RealtimeEventSender {
    pub fn new(
        senders: Vec<Sender<ChannelEvent>>,
        max_nps: Arc<ReadWriteAtomicU64>,
        ignore_range: RangeInclusive<u8>,
    ) -> RealtimeEventSender {
        RealtimeEventSender {
            senders: senders
                .into_iter()
                .map(|s| EventSender::new(max_nps.clone(), s, ignore_range.clone()))
                .collect(),
        }
    }

    pub fn send_event(&mut self, event: SynthEvent) {
        match event {
            SynthEvent::Channel(channel, event) => {
                self.senders[channel as usize].send_audio(event);
            }
            SynthEvent::AllChannels(event) => {
                for sender in self.senders.iter_mut() {
                    sender.send_audio(event.clone());
                }
            }
            SynthEvent::ChannelConfig(event) => {
                for sender in self.senders.iter_mut() {
                    sender.send_config(event.clone());
                }
            }
        }
    }

    pub fn send_config(&mut self, event: ChannelConfigEvent) {
        self.send_event(SynthEvent::ChannelConfig(event))
    }

    pub fn send_event_u32(&mut self, event: u32) {
        let head = event & 0xFF;
        let channel = head & 0xF;
        let code = head >> 4;

        macro_rules! val1 {
            () => {
                (event >> 8) as u8
            };
        }

        macro_rules! val2 {
            () => {
                (event >> 16) as u8
            };
        }

        match code {
            0x8 => {
                self.send_event(SynthEvent::Channel(
                    channel,
                    ChannelAudioEvent::NoteOff { key: val1!() },
                ));
            }
            0x9 => {
                self.send_event(SynthEvent::Channel(
                    channel,
                    ChannelAudioEvent::NoteOn {
                        key: val1!(),
                        vel: val2!(),
                    },
                ));
            }
            0xB => {
                self.send_event(SynthEvent::Channel(
                    channel,
                    ChannelAudioEvent::Control(ControlEvent::Raw(val1!(), val2!())),
                ));
            }
            0xC => {
                self.send_event(SynthEvent::Channel(
                    channel,
                    ChannelAudioEvent::ProgramChange(val1!()),
                ));
            }
            0xE => {
                let value = (((val2!() as i16) << 7) | val1!() as i16) - 8192;
                let value = value as f32 / 8192.0;
                self.send_event(SynthEvent::Channel(
                    channel,
                    ChannelAudioEvent::Control(ControlEvent::PitchBendValue(value)),
                ));
            }

            _ => {}
        }
    }

    pub fn reset_synth(&mut self) {
        self.send_event(SynthEvent::AllChannels(ChannelAudioEvent::AllNotesKilled));

        for sender in &mut self.senders {
            for i in 0..128 {
                sender.skipped_notes[i] = 0;
            }
        }

        self.send_event(SynthEvent::AllChannels(ChannelAudioEvent::ResetControl));
    }
}
