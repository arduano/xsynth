#![allow(non_snake_case)]

use std::{
    ffi::c_void,
    os::raw::c_ulong,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    path::PathBuf,
    collections::HashMap,
};

use core::{
    channel::{ChannelConfigEvent, ChannelInitOptions},
    soundfont::{SampleSoundfont, SoundfontBase},
};

use realtime::{config::XSynthRealtimeConfig, RealtimeEventSender, RealtimeSynth};

#[cfg(windows)]
use winapi::{
    shared::{basetsd::DWORD_PTR, minwindef::DWORD, windef::HWND},
    um::{
        mmsystem::{
            CALLBACK_EVENT, CALLBACK_FUNCTION, CALLBACK_THREAD, CALLBACK_WINDOW, HMIDI, HMIDIOUT,
        },
        synchapi::SetEvent,
        winnt::HANDLE,
        winuser::{IsWindow, PostMessageW, PostThreadMessageW},
    },
};

mod sf_list;
pub use sf_list::*;

struct Synth {
    killed: Arc<Mutex<bool>>,
    stats_join_handle: thread::JoinHandle<()>,

    senders: RealtimeEventSender,

    // This field is necessary to keep the synth loaded
    _synth: RealtimeSynth,
}

static mut GLOBAL_SYNTH: Option<Synth> = None;
static mut SOUNDFONTS: Option<HashMap<PathBuf, Arc<SampleSoundfont>>> = None;
static mut CURRENT_VOICE_COUNT: u64 = 0;

// region: Custom KDMAPI functions

// This function is not part of the KDMAPI standard.
#[no_mangle]
pub extern "C" fn GetVoiceCount() -> u64
{
    unsafe {
        CURRENT_VOICE_COUNT
    }
}
// endregion

// region: KDMAPI functions

#[no_mangle]
pub extern "C" fn InitializeKDMAPIStream() -> i32 {
    let channel_init_options = ChannelInitOptions {
        fade_out_killing: true,
        ..Default::default()
    };

    let config = XSynthRealtimeConfig {
        render_window_ms: 5.0,
        channel_init_options,
        ..Default::default()
    };

    let realtime_synth = RealtimeSynth::open_with_default_output(config);
    let mut sender = realtime_synth.get_senders();
    let params = realtime_synth.stream_params();

    let soundfonts: HashMap<PathBuf, Arc<SampleSoundfont>> = if let Some(soundfonts) = unsafe{ SOUNDFONTS.clone() } {
        soundfonts
    } else {
        let sfpaths = parse_list().unwrap_or(Vec::new());
        let mut soundfonts = HashMap::new();
        for path in sfpaths {
            let soundfont = Arc::new(SampleSoundfont::new(path.clone(), params, Default::default()).unwrap());
            if !soundfonts.contains_key(&path) {
                soundfonts.insert(path, soundfont);
            }
        }
        unsafe {
            SOUNDFONTS = Some(soundfonts.clone());
        }
        soundfonts
    };

    let mut sfs: Vec<Arc<dyn SoundfontBase>> = Vec::new();
    for soundfont in &soundfonts {
        sfs.push(soundfont.1.clone());
    }
    sender.send_config(ChannelConfigEvent::SetSoundfonts(sfs));

    let killed = Arc::new(Mutex::new(false));

    let stats = realtime_synth.get_stats();

    let killed_thread = killed.clone();
    let stats_join_handle = thread::spawn(move || {
        while !*killed_thread.lock().unwrap() {
            unsafe {
                CURRENT_VOICE_COUNT = stats.voice_count();
            }
            thread::sleep(Duration::from_millis(10));
        }
    });

    unsafe {
        GLOBAL_SYNTH = Some(Synth {
            killed,
            senders: sender,
            stats_join_handle,
            _synth: realtime_synth,
        });
    }
    1
}

#[no_mangle]
pub extern "C" fn TerminateKDMAPIStream() -> i32 {
    unsafe {
        if let Some(synth) = GLOBAL_SYNTH.take() {
            *synth.killed.lock().unwrap() = true;
            synth.stats_join_handle.join().ok();
        }
    }
    1
}

#[no_mangle]
pub extern "C" fn ResetKDMAPIStream() {
    unsafe {
        if let Some(synth) = GLOBAL_SYNTH.as_mut() {
            synth.senders.reset_synth();
        }
    }
}

#[no_mangle]
pub extern "C" fn SendDirectData(dwMsg: u32) -> u32 {
    unsafe {
        if let Some(sender) = GLOBAL_SYNTH.as_mut() {
            sender.senders.send_event_u32(dwMsg);
        }
    }
    1
}

#[no_mangle]
pub extern "C" fn SendDirectDataNoBuf(dwMsg: u32) -> u32 {
    SendDirectData(dwMsg); //We don't have a buffer, just use SendDirectData
    1
}

#[no_mangle]
pub extern "C" fn IsKDMAPIAvailable() -> u32 {
    1
}

// endregion

// region: Unimplemented functions

#[no_mangle]
pub extern "C" fn DisableFeedbackMode() {
    println!("DisableFeedbackMode");
}

#[no_mangle]
pub extern "C" fn SendCustomEvent(_eventtype: u32, _chan: u32, _param: u32) -> u32 {
    println!("SendCustomEvent");
    1
}

#[no_mangle]
pub extern "C" fn SendDirectLongData() -> u32 {
    println!("SendDirectLongData");
    1
}

#[no_mangle]
pub extern "C" fn SendDirectLongDataNoBuf() -> u32 {
    println!("SendDirectLongDataNoBuf");
    1
}

#[no_mangle]
pub extern "C" fn PrepareLongData() -> u32 {
    println!("PrepareLongData");
    1
}

#[no_mangle]
pub extern "C" fn UnprepareLongData() -> u32 {
    println!("UnprepareLongData");
    1
}

#[no_mangle]
pub extern "C" fn DriverSettings(
    _dwParam: c_ulong,
    _dwCmd: c_ulong,
    _lpValue: *mut c_void,
    _cbSize: c_ulong,
) -> u32 {
    println!("DriverSettings");
    1
}

#[no_mangle]
pub extern "C" fn LoadCustomSoundFontsList(_Directory: u16) {
    println!("LoadCustomSoundFontsList");
}

#[no_mangle]
pub extern "C" fn GetDriverDebugInfo() {
    println!("GetDriverDebugInfo");
}

// endregion

// region: Callback functions for WINMM Wrapper (Windows Only)

cfg_if::cfg_if! {
  if #[cfg(windows)] {
    type CallbackFunction = unsafe extern "C" fn(HMIDIOUT, DWORD, DWORD_PTR, DWORD_PTR, DWORD_PTR);
    unsafe extern "C" fn def_callback(_: HMIDIOUT, _: DWORD, _: DWORD_PTR, _: DWORD_PTR, _: DWORD_PTR) {
    }
    static mut DUMMY_DEVICE: HMIDI = std::ptr::null_mut();
    static mut CALLBACK_INSTANCE: DWORD_PTR = 0;
    static mut CALLBACK: CallbackFunction = def_callback;
    static mut CALLBACK_TYPE: DWORD = 0;

    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn ReturnKDMAPIVer(
        Major: *mut c_ulong,
        Minor: *mut c_ulong,
        Build: *mut c_ulong,
        Revision: *mut c_ulong,
    ) -> u32 {
        println!("ReturnKDMAPIVer");
        *Major = 4;
        *Minor = 1;
        *Build = 0;
        *Revision = 5;
        1
    }

    #[no_mangle]
    pub extern "C" fn timeGetTime64() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[no_mangle]
    pub extern "C" fn modMessage() -> u32 {
        println!("modMessage");
        1
    }

    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn InitializeCallbackFeatures(
        OMHM: HMIDI,
        OMCB: CallbackFunction,
        OMI: DWORD_PTR,
        _OMU: DWORD_PTR,
        OMCM: DWORD,
    ) -> u32 {
        println!("InitializeCallbackFeatures");

        DUMMY_DEVICE = OMHM;
        CALLBACK = OMCB;
        CALLBACK_INSTANCE = OMI;
        CALLBACK_TYPE = OMCM;

        #[allow(clippy::fn_address_comparisons)]
        if OMCM == CALLBACK_WINDOW && CALLBACK != def_callback && IsWindow(CALLBACK as HWND) != 0 {
            return 0;
        }

        1
    }

    #[no_mangle]
    #[allow(clippy::missing_safety_doc)]
    pub unsafe extern "C" fn RunCallbackFunction(Msg: DWORD, P1: DWORD_PTR, P2: DWORD_PTR) {
        println!("RunCallbackFunction");

        //We do a match case just to support stuff if needed
        match CALLBACK_TYPE {
            CALLBACK_FUNCTION => {
                CALLBACK(DUMMY_DEVICE as HMIDIOUT, Msg, P1, P2, CALLBACK_INSTANCE);
            }
            CALLBACK_EVENT => {
                SetEvent(CALLBACK as HANDLE);
            }
            CALLBACK_THREAD => {
                #[allow(clippy::fn_to_numeric_cast_with_truncation)]
                PostThreadMessageW(CALLBACK as DWORD, Msg, P1, P2.try_into().unwrap());
            }
            CALLBACK_WINDOW => {
                PostMessageW(CALLBACK as HWND, Msg, P1, P2.try_into().unwrap());
            }
            _ => println!("Type was NULL, Do Nothing"),
        }
    }
  }
}

// endregion
