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
    level_start_time: Option<f64>,
    level_sounds: Option<LevelSounds>,
    collide_beat: Option<(i32, f64)>,
}

impl State {
    fn instruments(&self, level: i32) -> Vec<&ceptre::Phrase> {
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

    fn beat_pos_for_time(&self, time: f64) -> f32 {
        let sounds = self.level_sounds.as_ref().expect("level_sounds");
        let frac = ((time - self.level_start_time.expect("level_start_time")) as f32
            / ray::get_music_time_length(sounds.metronome)) % 1.0;

        16.0 * frac
    }
}

struct LevelSounds {
    level: i32,
    metronome: ray::Music,
    instruments: Vec<InstrumentSound>,
}

struct InstrumentSound {
    number: i32,
    sequence: Vec<bool>,
    // sounds are named with sequence starting from beat 0,
    // but are recorded from the first beat that is non-empty
    sound: ray::Music,
}

static mut STATE: Option<State> = None;

fn main() {
    ray::init_window(WIDTH, HEIGHT, "ld42");
    ray::init_audio_device();

    let state = State {
        time: ray::get_time(),
        ceptre_context: ceptre::Context::from_text(include_str!("main.ceptre")),
        level_start_time: None,
        level_sounds: None,
        collide_beat: None,
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

    let time1 = state.time;
    let time2 = ray::get_time();
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
        .map(|p| i32::from_str(p[1].as_str(&state.ceptre_context.string_cache)).unwrap())
        .expect("current_level");

    // create sounds
    match state.level_sounds {
        Some(LevelSounds { level, .. }) if level == current_level => (),
        _ => {
            let metronome = ray::load_music_stream(&format!(
                "assets/level{} metronome.ogg",
                current_level + 1,
            ));

            ray::set_music_volume(metronome, 0.7);
            ray::set_music_loop_count(metronome, 0);

            let instruments = state
                .instruments(current_level)
                .iter()
                .map(|instrument| {
                    let number = i32::from_str(
                        instrument[2].as_str(&state.ceptre_context.string_cache),
                    ).unwrap();

                    let o = state.ceptre_context.to_existing_atom("o").expect("o");
                    let x = state.ceptre_context.to_existing_atom("x").expect("x");

                    let mut sequence = instrument[3..instrument.len()]
                        .iter()
                        .map(|v| match &v.string {
                            v if *v == o => false,
                            v if *v == x => true,
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>();

                    while sequence.len() < 16 {
                        sequence.push(false);
                    }

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

                    ray::set_music_volume(sound, 0.7);
                    ray::set_music_loop_count(sound, 0);

                    InstrumentSound {
                        number,
                        sequence,
                        sound,
                    }
                })
                .collect::<Vec<_>>();

            state.level_start_time = Some(state.time);

            state.level_sounds = Some(LevelSounds {
                level: current_level,
                metronome,
                instruments,
            })
        }
    };

    let beat_pos = state.beat_pos_for_time(state.time);

    let is_new_bar = state.beat_pos_for_time(time1) > beat_pos;
    let is_new_beat = state.beat_pos_for_time(time1).floor() as i32 != beat_pos.floor() as i32;

    if let Some(ref sounds) = state.level_sounds {
        // start metronome on first loop
        if beat_pos < 1.0 && !ray::is_music_playing(sounds.metronome) {
            ray::play_music_stream(sounds.metronome);
        }

        // restart metronome
        if is_new_bar {
            if ray::is_music_playing(sounds.metronome) {
                ray::stop_music_stream(sounds.metronome);
            }
            ray::play_music_stream(sounds.metronome);
        }

        // restart music
        if is_new_beat {
            for note in state
                .ceptre_context
                .find_phrases4(
                    Some("note"),
                    None,
                    Some(&beat_pos.floor().to_string()),
                    Some("first"),
                )
                .iter()
            {
                let instrument = i32::from_str(note[1].as_str(&state.ceptre_context.string_cache))
                    .expect("instrument");

                let sound = sounds
                    .instruments
                    .iter()
                    .find(|i| i.number == instrument)
                    .expect("instrument")
                    .sound;

                if ray::is_music_playing(sound) {
                    ray::stop_music_stream(sound);
                }
                ray::play_music_stream(sound);
            }
        }
    }

    let instrument_count = state.instruments(current_level).len() as i32;

    {
        let mut has_notes = vec![];
        for _ in 0..instrument_count {
            has_notes.push(false);
        }

        for note in state
            .ceptre_context
            .find_phrases(Some("note"))
            .iter()
            .chain(state.ceptre_context.find_phrases(Some("note-tmp")).iter())
        {
            let instrument = i32::from_str(note[1].as_str(&state.ceptre_context.string_cache))
                .expect("instrument") as usize;

            has_notes[instrument] = true;
        }

        for (instrument, has_notes) in has_notes.iter().enumerate() {
            if !has_notes {
                let level_sounds = state.level_sounds.as_ref().expect("level_sounds");
                let sound = level_sounds
                    .instruments
                    .iter()
                    .find(|i| i.number == instrument as i32)
                    .expect("instrument")
                    .sound;

                if ray::is_music_playing(sound) {
                    ray::stop_music_stream(sound);
                }
            }
        }
    }

    state
        .ceptre_context
        .append_state(&format!("#set-beat {}", beat_pos.floor() as i32));

    if ray::is_key_pressed(ray::KEY_SPACE) {
        state
            .ceptre_context
            .append_state(&format!("#input-place {}", beat_pos.floor() as i32));

        let selected_instrument = state.selected_instrument().expect("selected_instrument");

        let level_sounds = state.level_sounds.as_ref().expect("level_sounds");
        let sound = level_sounds.instruments[selected_instrument as usize].sound;

        if ray::is_music_playing(sound) {
            ray::stop_music_stream(sound);
        }
        ray::play_music_stream(sound);
    }

    if ray::is_key_pressed(ray::KEY_LEFT) {
        state.ceptre_context.append_state("#input-change-left");
    } else if ray::is_key_pressed(ray::KEY_RIGHT) {
        state.ceptre_context.append_state("#input-change-right");
    };

    // state.ceptre_context.print();

    {
        let collide_atom = state.ceptre_context.to_atom("^collide");
        let mut collide_pos_atom = None;

        ceptre::update(&mut state.ceptre_context, |p: &ceptre::Phrase| {
            if collide_atom == p[0].string && collide_pos_atom.is_none() {
                collide_pos_atom = Some(p[1].string);
            }

            None
        });

        if let Some(pos) = collide_pos_atom.map(|a| {
            let s = state.ceptre_context.string_cache.from_atom(a);
            i32::from_str(s).expect("pos")
        }) {
            state.collide_beat = Some((pos, state.time));
        }
    }

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

    {
        let x = min_x + (beat_pos.floor() as i32) * note_width;
        let y = min_y;
        let color = ray::WHITE;
        ray::draw_rectangle_lines(x, y, note_width, note_height, color);
    }

    for note in state.ceptre_context.find_phrases(Some("note-tmp")).iter() {
        let string_cache = &state.ceptre_context.string_cache;
        let instrument = i32::from_str(note[1].as_str(string_cache)).expect("instrument");
        let pos = i32::from_str(note[2].as_str(string_cache)).expect("pos");

        let x = min_x + pos * note_width;
        let y = min_y;
        let thickness = 4;
        let color = instrument_color(instrument);

        ray::draw_rectangle_lines_ex(
            ray::Rectangle {
                x: x as f32,
                y: y as f32,
                width: note_width as f32,
                height: note_height as f32,
            },
            thickness,
            color,
        );
    }

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

    if let Some((pos, time)) = state.collide_beat {
        let frac = ((state.time - time) as f32 / 0.567).min(1.0);

        let x = (min_x + pos * note_width) as f32 + note_width as f32 * 0.5;
        let y = min_y as f32 + note_height as f32 * 0.5;

        let width = note_width as f32 * (1.0 + frac * 0.5);
        let height = note_height as f32 * (1.0 + frac * 0.5);

        let origin = ray::Vector2 {
            x: width * 0.5,
            y: height * 0.5,
        };

        let color = ray::Color {
            r: 255,
            g: 0,
            b: 0,
            a: (255.0 * (1.0 - frac)) as u8,
        };

        ray::draw_rectangle_pro(
            ray::Rectangle {
                x: x as f32,
                y: y as f32,
                width,
                height,
            },
            origin,
            0.0,
            color,
        );
    }

    ray::end_drawing();
}

fn beat_pos_for_sound(sound: &ray::Music) -> f32 {
    let played = ray::get_music_time_played(*sound);
    let length = ray::get_music_time_length(*sound);

    16.0 * (played / length)
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
