//#![windows_subsystem = "windows"]

#[macro_use]
extern crate imgui;

use sdl2::{
    image::InitFlag,
    messagebox::{self, MessageBoxFlag},
    video::{gl_attr::GLAttr, GLContext, Window as SdlWindow},
    Sdl, VideoSubsystem,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{error::Error, fs, path::Path, process, str, time::Instant};

mod editor;
use editor::*;
use sdlglue::Renderer;
//use wee::*;
use wee::*;
use wee_common::{Colour, WeeResult};

fn init_logger() {
    if let Err(error) = simple_logger::init() {
        eprintln!("Could not initialise logger");
        eprintln!("{}", error)
    }
}

trait GlContextFromSdl {
    fn from_sdl(video_subsystem: &GLAttr, window: &SdlWindow) -> Self;
}

impl GlContextFromSdl for sdl2::video::GLContext {
    fn from_sdl(gl_attr: &GLAttr, window: &SdlWindow) -> GLContext {
        gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
        gl_attr.set_context_version(3, 0);

        window
            .gl_create_context()
            .expect("Couldn't create GL context")
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct PlaybackConfig {
    min: f32,
    max: f32,
    increase: f32,
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    intro_font: IntroFontConfig,
    ui_font: FontLoadInfo,
    playback_rate: PlaybackConfig,
    volume: f32,
    render_each_frame: bool,
    sensitivity: f32,
}

fn yaml_from_str<T: DeserializeOwned>(text: &str) -> WeeResult<T> {
    match serde_yaml::from_str(text) {
        Ok(data) => Ok(data),
        Err(error) => Err(Box::new(error)),
    }
}

impl Config {
    fn from_file<P: AsRef<Path>>(path: P) -> WeeResult<Config> {
        let yaml = fs::read_to_string(path)?;

        yaml_from_str(&yaml)
    }

    fn settings(&self) -> GameSettings {
        GameSettings {
            volume: self.volume,
            render_each_frame: self.render_each_frame,
        }
    }
}

enum GameMode {
    Edit,
    Error(Box<dyn Error + Send + Sync>),
}

#[derive(Debug, Copy, Clone)]
struct LastGame {
    has_won: bool,
    was_life_gained: bool,
}

trait MessageBoxError {
    fn show_error(self) -> Self;
}

impl<T> MessageBoxError for WeeResult<T> {
    fn show_error(self) -> Self {
        if let Err(error) = &self {
            messagebox::show_simple_message_box(
                MessageBoxFlag::ERROR,
                "Error",
                &error.to_string(),
                None,
            )
            .unwrap();
        }
        self
    }
}

impl<T> MessageBoxError for Result<T, String> {
    fn show_error(self) -> Self {
        if let Err(error) = &self {
            messagebox::show_simple_message_box(MessageBoxFlag::ERROR, "Error", &error, None)
                .unwrap();
        }
        self
    }
}

fn run_main_loop(
    mut game_mode: GameMode,
    renderer: &mut Renderer,
    events: &mut EventState,
    imgui: &mut Imgui,
    intro_font: &FontSystem,
    config: &Config,
) -> WeeResult<GameMode> {
    match game_mode {
        GameMode::Edit => {
            let was_fullscreen = !matches!(
                renderer.window.fullscreen_state(),
                sdl2::video::FullscreenType::Off
            );

            renderer.exit_fullscreen(&events.mouse.utils)?;
            events.mouse.utils.set_relative_mouse_mode(false);
            editor::run(renderer, events, imgui, intro_font, config.settings())?;
            if was_fullscreen {
                renderer.enter_fullscreen(&events.mouse.utils)?;
            }
            process::exit(0);
        }

        GameMode::Error(error) => {
            let mut last_frame = Instant::now();
            let mut do_break = false;
            'error_running: loop {
                if sdlglue::has_quit(&mut events.pump) {
                    process::exit(0);
                }
                renderer.adjust_fullscreen(&events.pump, &events.mouse.utils)?;
                //event_state.update_mouse();
                sdlglue::clear_screen(Colour::dull_grey());

                let imgui_frame = imgui.prepare_frame(
                    &renderer.window,
                    &events.pump.mouse_state(),
                    &mut last_frame,
                );
                let ui = &imgui_frame.ui;

                imgui::Window::new(im_str!("Error")).build(ui, || {
                    ui.text(format!("{}", error));
                    if ui.button(im_str!("OK"), [100.0, 50.0]) {
                        do_break = true;
                    }
                    ui.same_line(0.0);
                    if ui.button(im_str!("Quit"), [100.0, 50.0]) {
                        process::exit(0);
                    }
                });

                if do_break {
                    break 'error_running;
                }

                imgui_frame.render(&renderer.window);

                renderer.draw_mouse(events.mouse.position);

                renderer.present();
            }

            game_mode = GameMode::Edit;
        }
    }
    Ok(game_mode)
}

struct MainGame {
    game_mode: GameMode,
    renderer: Renderer,
    events: EventState,
    imgui: Imgui,
    config: Config,
}

impl MainGame {
    fn init(
        config: Config,
        sdl_context: &Sdl,
        video_subsystem: &VideoSubsystem,
        window: SdlWindow,
    ) -> WeeResult<MainGame> {
        let imgui = Imgui::init(&config.ui_font, &video_subsystem, &window)?;

        let event_pump = sdl_context.event_pump()?;
        let events = EventState {
            pump: event_pump,
            mouse: MouseState::new(config.sensitivity, sdl_context.mouse()),
        };

        let mouse_texture = sdlglue::Texture::from_file_with_filtering("mouse.png")?;

        let renderer = Renderer::new(window, mouse_texture);

        let game_mode = GameMode::Edit;

        Ok(MainGame {
            game_mode,
            renderer,
            events,
            imgui,
            config,
        })
    }

    fn run(mut self, font_system: &FontSystem) {
        /*editor::run(
            &mut self.renderer,
            &mut self.events,
            &mut self.imgui,
            font_system,
            self.config.settings(),
        )
        .unwrap();*/

        loop {
            let loop_result = run_main_loop(
                self.game_mode,
                &mut self.renderer,
                &mut self.events,
                &mut self.imgui,
                font_system,
                &self.config,
            );
            self.game_mode = match loop_result {
                Ok(game_mode) => game_mode,
                Err(error) => GameMode::Error(error),
            };
        }
    }
}

fn main() -> WeeResult<()> {
    init_logger();

    let config = Config::from_file("config.yaml")
        .map_err(|error| format!("Error loading configuration from config.yaml.\n{}", error))
        .show_error()?;

    let sdl_context = sdl2::init().show_error()?;

    let video_subsystem = sdl_context.video().show_error()?;

    let _image_context =
        sdl2::image::init(InitFlag::PNG | InitFlag::JPG | InitFlag::WEBP).show_error()?;

    let window = {
        const INITIAL_WINDOW_WIDTH: u32 = 1200;
        const INITIAL_WINDOW_HEIGHT: u32 = 675;

        video_subsystem
            .window("Wee", INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT)
            .position_centered()
            //.fullscreen()
            .opengl()
            .resizable()
            .build()
            .map_err(|e| e.to_string())
            .show_error()?
    };

    sdl_context.mouse().set_relative_mouse_mode(false);

    let _gl_context = GLContext::from_sdl(&video_subsystem.gl_attr(), &window);

    gl::load_with(|s| video_subsystem.gl_get_proc_address(s) as _);

    unsafe {
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    }

    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).show_error()?;

    let font_system = FontSystem::load(&config.intro_font, &ttf_context).show_error()?;

    MainGame::init(config, &sdl_context, &video_subsystem, window)
        .show_error()?
        .run(&font_system);

    Ok(())
}
