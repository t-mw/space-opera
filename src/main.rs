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

const WIDTH: i32 = 640;
const HEIGHT: i32 = 480;

struct State {
    time: f64,
    ceptre_context: ceptre::Context,
    level_sounds: Option<LevelSounds>,
}

impl State {
    fn instruments(&self, level: usize) -> Vec<&ceptre::Phrase> {
        self.ceptre_context
            .find_phrases2(Some("level-instruments"), Some(&level.to_string()))
    }

    fn selected_instrument(&self) -> Option<i32> {
        self.ceptre_context
            .find_phrase(Some("selected-instrument"))
            .map(|p| {
                i32::from_str(p[1].as_str(&self.ceptre_context.string_cache))
                    .expect("selected_instrument")
            })
    }
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
    ray::init_window(WIDTH, HEIGHT, "ld42");
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

    let current_level = state
        .ceptre_context
        .find_phrase(Some("current-level"))
        .map(|p| usize::from_str(p[1].as_str(&state.ceptre_context.string_cache)).unwrap())
        .expect("current_level");

    // create sounds
    match state.level_sounds {
        Some(LevelSounds { level, .. }) if level == current_level => (),
        _ => {
            let metronome = ray::load_music_stream(&format!(
                "assets/level{} metronome.ogg",
                current_level + 1,
            ));

            let instruments = state
                .instruments(current_level)
                .iter()
                .map(|instrument| {
                    let number = usize::from_str(
                        instrument[2].as_str(&state.ceptre_context.string_cache),
                    ).unwrap();

                    let o = state.ceptre_context.to_existing_atom("o").expect("o");
                    let x = state.ceptre_context.to_existing_atom("x").expect("x");

                    let sequence = instrument[3..instrument.len() - 1]
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
                        current_level + 1,
                        number + 1,
                        sequence_str
                    ));

                    InstrumentSound {
                        number,
                        sequence,
                        sound,
                    }
                })
                .collect::<Vec<_>>();

            ray::play_music_stream(metronome);

            state.level_sounds = Some(LevelSounds {
                level: current_level,
                metronome,
                instruments,
            })
        }
    };

    if ray::is_key_pressed(ray::KEY_SPACE) {
        state.ceptre_context.append_state("#input-place");

        let selected_instrument = state.selected_instrument().expect("selected_instrument");

        let level_sounds = state.level_sounds.as_ref().expect("level_sounds");
        ray::play_music_stream(level_sounds.instruments[selected_instrument as usize].sound);
    }

    if ray::is_key_pressed(ray::KEY_LEFT) {
        state.ceptre_context.append_state("#input-change-left");
    } else if ray::is_key_pressed(ray::KEY_RIGHT) {
        state.ceptre_context.append_state("#input-change-right");
    };

    // state.ceptre_context.print();

    ceptre::update(&mut state.ceptre_context, |p: &ceptre::Phrase| None);

    ray::begin_drawing();

    ray::clear_background(ray::BLACK);

    let instrument_color = |idx: i32| [ray::BLUE, ray::RED, ray::ORANGE, ray::PURPLE][idx as usize];

    let min_x = 40;
    let max_x = WIDTH - min_x;
    let min_y = 100;
    let max_y = HEIGHT - min_y;
    let note_width = (max_x - min_x) / 16;
    let note_height = max_y - min_y;

    for note in state.ceptre_context.find_phrases(Some("note")).iter() {
        let string_cache = &state.ceptre_context.string_cache;
        let instrument = i32::from_str(note[1].as_str(string_cache)).expect("instrument");
        let pos = i32::from_str(note[2].as_str(string_cache)).expect("pos");

        let x = min_x + pos * note_width;
        let y = min_y;
        let color = instrument_color(instrument);
        ray::draw_rectangle(x, y, note_width, note_height, color);
    }

    if let Some(ref sounds) = state.level_sounds {
        let played = ray::get_music_time_played(sounds.metronome);
        let length = ray::get_music_time_length(sounds.metronome);

        let pos = (16.0 * (played / length)) as i32;
        let x = min_x + pos * note_width;
        let y = min_y;
        let color = ray::WHITE;
        ray::draw_rectangle_lines(x, y, note_width, note_height, color);
    }

    let instrument_count = state.instruments(current_level).len() as i32;
    for i in 0..instrument_count {
        let min_x = 100;
        let max_x = WIDTH - min_x;

        let x = min_x + i * (max_x - min_x) / (instrument_count - 1);
        let y = 50;
        let radius = 10.0;

        let selected_instrument = state.selected_instrument().expect("selected_instrument");
        if i == selected_instrument {
            ray::draw_circle(x, y, radius + 4.0, instrument_color(i));
        }

        ray::draw_circle(x, y, radius, ray::WHITE);
    }

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
