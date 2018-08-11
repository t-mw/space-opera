extern crate raylib_rs;
use raylib_rs as ray;

use std::cell::RefCell;
use std::os::raw::{c_int, c_void};
use std::ptr::null_mut;

struct State {
    music: ray::Music,
}

fn main() {
    ray::init_window(640, 480, "ld42");
    ray::init_audio_device();

    let mut state = State {
        music: ray::load_music_stream("assets/music.mod"),
    };

    ray::play_music_stream(state.music);

    if cfg!(target_os = "emscripten") {
        set_main_loop_callback(|| update_draw_frame(&mut state));
    } else {
        while !ray::window_should_close() {
            update_draw_frame(&mut state);
        }
    }

    ray::close_window();
}

fn update_draw_frame(state: &mut State) {
    ray::update_music_stream(state.music);

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
