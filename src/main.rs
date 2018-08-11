#[macro_use]
extern crate lazy_static;
extern crate rand;
extern crate raylib_rs;
extern crate regex;

#[macro_use]
mod dump;
mod ceptre;

use raylib_rs as ray;

use std::cell::RefCell;
use std::os::raw::{c_int, c_void};
use std::ptr::null_mut;
use std::str::FromStr;
use std::vec::Vec;

struct State {
    time: f64,
    ceptre_context: ceptre::Context,
    level_sounds: Option<LevelSounds>,
}

struct LevelSounds {
    level: usize,
    metronome: ray::Music,
    instruments: Vec<InstrumentSound>,
}

struct InstrumentSound {
    number: usize,
    sequence: Vec<bool>,
    sound: ray::Music,
}

static mut STATE: Option<State> = None;

fn main() {
    ray::init_window(640, 480, "ld42");
    ray::init_audio_device();

    let state = State {
        time: ray::get_time(),
        ceptre_context: ceptre::Context::from_text(include_str!("main.ceptre")),
        level_sounds: None,
    };

    unsafe { STATE = Some(std::mem::transmute(state)) };

    if cfg!(target_os = "emscripten") {
        set_main_loop_callback(|| update_draw_frame());
    } else {
        while !ray::window_should_close() {
            update_draw_frame();
        }
    }
}

fn update_draw_frame() {
    let state = unsafe { STATE.as_mut().unwrap() };

    let time1 = ray::get_time();
    let time2 = state.time;
    let dt = (time2 - time1) as f32;
    state.time = time2;

    if let Some(ref sounds) = state.level_sounds {
        ray::update_music_stream(sounds.metronome);
        for instrument in sounds.instruments.iter() {
            ray::update_music_stream(instrument.sound);
        }
    }

    ceptre::update(&mut state.ceptre_context, |p: &ceptre::Phrase| None);

    let current_level = state
        .ceptre_context
        .find_phrase(Some("current-level"))
        .map(|p| usize::from_str(p[1].as_str(&state.ceptre_context.string_cache)).unwrap())
        .expect("current_level");

    // create sounds
    match state.level_sounds {
        Some(LevelSounds { level, .. }) if level == current_level => (),
        _ => {
            let metronome =
                ray::load_music_stream(&format!("assets/level{} metronome.ogg", current_level,));

            let instruments = state
                .ceptre_context
                .find_phrases2(Some("level-instruments"), Some(&current_level.to_string()))
                .iter()
                .map(|instrument| {
                    let number = usize::from_str(
                        instrument[2].as_str(&state.ceptre_context.string_cache),
                    ).unwrap();

                    let o = state.ceptre_context.to_existing_atom("o").expect("o");
                    let x = state.ceptre_context.to_existing_atom("x").expect("x");

                    let sequence = instrument[3..]
                        .iter()
                        .map(|v| match &v.string {
                            v if *v == o => false,
                            v if *v == x => true,
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>();

                    let sequence_str = sequence
                        .iter()
                        .map(|v| if *v { "1" } else { "0" })
                        .collect::<Vec<_>>()
                        .join("");

                    let sound = ray::load_music_stream(&format!(
                        "assets/level{} {}-{}.ogg",
                        current_level, number, sequence_str
                    ));

                    InstrumentSound {
                        number,
                        sequence,
                        sound,
                    }
                })
                .collect::<Vec<_>>();

            ray::play_music_stream(metronome);
            for instrument in instruments.iter() {
                ray::play_music_stream(instrument.sound);
            }

            state.level_sounds = Some(LevelSounds {
                level: current_level,
                metronome,
                instruments,
            })
        }
    };

    ray::begin_drawing();

    ray::clear_background(ray::BLACK);
    ray::draw_text("Hello, world!", 12, 12, 20, ray::WHITE);

    ray::end_drawing();
}

extern "C" {
    pub fn emscripten_set_main_loop(m: unsafe extern "C" fn(), fps: c_int, infinite: c_int);
}

thread_local!(static MAIN_LOOP_CALLBACK: RefCell<*mut c_void> = RefCell::new(null_mut()));

pub fn set_main_loop_callback<F>(callback: F)
where
    F: FnMut(),
{
    MAIN_LOOP_CALLBACK.with(|log| {
        *log.borrow_mut() = &callback as *const _ as *mut c_void;
    });

    unsafe {
        emscripten_set_main_loop(wrapper::<F>, 0, 1);
    }
}

unsafe extern "C" fn wrapper<F>()
where
    F: FnMut(),
{
    MAIN_LOOP_CALLBACK.with(|z| {
        let closure = *z.borrow_mut() as *mut F;
        (*closure)();
    });
}

#[cfg(target_os = "macos")]
mod mac {
    #[link(kind = "static", name = "raylib")]
    #[link(kind = "framework", name = "OpenGL")]
    #[link(kind = "framework", name = "Cocoa")]
    #[link(kind = "framework", name = "IOKit")]
    #[link(kind = "framework", name = "GLUT")]
    #[link(kind = "framework", name = "CoreVideo")]
    extern "C" {}
}
