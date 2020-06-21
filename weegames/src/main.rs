#![windows_subsystem = "windows"]

#[macro_use]
extern crate imgui;

use imgui_opengl_renderer::Renderer as ImguiRenderer;
use imgui_sdl2::ImguiSdl2 as ImguiSdl;
use rand::{seq::SliceRandom, thread_rng};
use sdl2::{
    image::InitFlag,
    messagebox::{self, MessageBoxFlag},
    video::{GLContext, Window as SdlWindow},
    EventPump, VideoSubsystem,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    error::Error,
    fs,
    path::Path,
    process, str,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

use sdlglue::Renderer;
use wee::{
    Completion, FontLoadInfo, GameData, GameSettings, IntroFont, IntroFontConfig, LoadedGame,
    SerialiseObject, Switch, DEFAULT_GAME_SPEED,
};
use wee_common::{Colour, WeeResult};

fn init_logger() {
    if let Err(error) = simple_logger::init() {
        eprintln!("Could not initialise logger");
        eprintln!("{}", error)
    }
}

trait GlContextFromSdl {
    fn from_sdl(video_subsystem: &VideoSubsystem, window: &SdlWindow) -> Self;
}

impl GlContextFromSdl for sdl2::video::GLContext {
    fn from_sdl(video_subsystem: &VideoSubsystem, window: &SdlWindow) -> GLContext {
        let gl_attr = video_subsystem.gl_attr();
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
}

impl Config {
    fn from_file<P: AsRef<Path>>(path: P) -> WeeResult<Config> {
        let yaml = fs::read_to_string(path)?;

        fn yaml_from_str<T: DeserializeOwned>(text: &str) -> WeeResult<T> {
            match serde_yaml::from_str(text) {
                Ok(data) => Ok(data),
                Err(error) => Err(Box::new(error)),
            }
        }

        yaml_from_str(&yaml)
    }

    fn settings(&self) -> GameSettings {
        GameSettings {
            volume: self.volume,
            render_each_frame: self.render_each_frame,
        }
    }
}

struct Imgui {
    context: imgui::Context,
    sdl: ImguiSdl,
    renderer: ImguiRenderer,
}

impl Imgui {
    fn init(
        config: &Config,
        video_subsystem: &VideoSubsystem,
        window: &SdlWindow,
    ) -> WeeResult<Imgui> {
        let mut context = {
            let mut context = imgui::Context::create();
            context.set_ini_filename(None);

            context.fonts().clear();

            let bytes = std::fs::read(&config.ui_font.filename)?;

            context.fonts().add_font(&[imgui::FontSource::TtfData {
                data: &bytes,
                size_pixels: config.ui_font.size as f32,
                config: None,
            }]);
            context
        };

        let imgui_sdl = imgui_sdl2::ImguiSdl2::new(&mut context, &window);

        let imgui_renderer = imgui_opengl_renderer::Renderer::new(&mut context, |s| {
            video_subsystem.gl_get_proc_address(s) as _
        });

        Ok(Imgui {
            context,
            sdl: imgui_sdl,
            renderer: imgui_renderer,
        })
    }

    fn prepare_frame(
        &mut self,
        renderer: &Renderer,
        event_pump: &EventPump,
        last_frame: &mut Instant,
    ) -> ImguiFrame {
        self.sdl.prepare_frame(
            self.context.io_mut(),
            &renderer.window,
            &event_pump.mouse_state(),
        );

        self.update_time(last_frame);

        ImguiFrame {
            ui: self.context.frame(),
            sdl: &mut self.sdl,
            renderer: &mut self.renderer,
        }
    }

    fn update_time(&mut self, last_frame: &mut Instant) {
        let imgui_now = Instant::now();
        let delta = imgui_now - *last_frame;
        let delta_s = delta.as_secs() as f32 + delta.subsec_nanos() as f32 / 1_000_000_000.0;
        *last_frame = imgui_now;
        self.context.io_mut().delta_time = delta_s;
    }
}

struct ImguiFrame<'a> {
    ui: imgui::Ui<'a>,
    sdl: &'a mut ImguiSdl,
    renderer: &'a mut ImguiRenderer,
}

impl<'a> ImguiFrame<'a> {
    fn render(self, renderer: &Renderer) {
        self.sdl.prepare_render(&self.ui, &renderer.window);
        self.renderer.render(self.ui);
    }
}

enum GameMode<'a, 'b> {
    Menu,
    Play {
        game: LoadedGame<'a, 'b>,
        games_list: GamesList,
        progress: Progress,
    },
    Edit,
    Prelude,
    Interlude {
        won: bool,
        games_list: GamesList,
        progress: Progress,
    },
    GameOver {
        progress: Progress,
    },
    Error(Box<dyn Error + Send + Sync>),
}

#[derive(Debug, Copy, Clone)]
struct Progress {
    playback_rate: f32,
    score: i32,
    lives: i32,
}

const MAX_LIVES: i32 = 4;

impl Progress {
    fn new(playback_rate: f32) -> Progress {
        Progress {
            playback_rate,
            score: 0,
            lives: MAX_LIVES,
        }
    }

    fn update(&mut self, has_won: bool, playback_config: &PlaybackConfig) {
        if has_won {
            self.score += 1;
            if self.score % 5 == 0 {
                self.playback_rate += playback_config.increase;
            }
            self.playback_rate = self.playback_rate.min(playback_config.max);
        } else {
            self.lives -= 1;
        }
    }
}

#[derive(Debug)]
struct GamesList(Vec<String>);

impl GamesList {
    fn new() -> WeeResult<GamesList> {
        let mut games_list = Vec::new();
        for entry in WalkDir::new("games").into_iter().filter_map(|e| e.ok()) {
            let metadata = entry.metadata()?;
            let right_extension = match entry.path().extension() {
                Some(ext) => ext == "json",
                None => false,
            };
            if right_extension && metadata.is_file() {
                let filename = entry.path().to_str().unwrap();
                log::info!("{}", filename);

                let game_data = GameData::load(filename);

                match game_data {
                    Ok(data) => {
                        if data.published {
                            games_list.push(filename.to_string());
                        }
                    }
                    Err(error) => {
                        log::warn!("Couldn't read {:?}", filename);
                        log::warn!("{}", error);
                    }
                }
            }
        }

        if games_list.is_empty() {
            let error = Box::<dyn Error + Send + Sync>::from("No games found");
            Err(error)
        } else {
            Ok(GamesList(games_list))
        }
    }

    fn choose(&self) -> String {
        self.0
            .choose(&mut thread_rng())
            .expect("Games list is empty")
            .to_string()
    }
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

fn replace_text(object: &mut SerialiseObject, progress: &Progress) {
    object.replace_text(progress.score, progress.lives);
}

fn run_main_loop<'a, 'b>(
    mut game_mode: GameMode<'a, 'b>,
    renderer: &mut Renderer,
    event_pump: &mut EventPump,
    imgui: &mut Imgui,
    intro_font: &'a IntroFont<'a, 'b>,
    config: &Config,
) -> WeeResult<GameMode<'a, 'b>> {
    match game_mode {
        GameMode::Menu => {
            let mut game = {
                let filename = "games/system/main-menu.json";

                LoadedGame::load(filename, &intro_font)
                    .map_err(|error| format!("Couldn't load {}\n{}", filename, error))?
                    .start(DEFAULT_GAME_SPEED, config.settings())
            };

            'menu_running: loop {
                sdlglue::set_fullscreen(renderer, event_pump)?;

                game.update_and_render_frame(renderer, event_pump)?;

                if wee::is_switched_on(&game.objects, "Play") {
                    game_mode = GameMode::Prelude;
                    break 'menu_running;
                }
                if wee::is_switched_on(&game.objects, "Edit") {
                    game_mode = GameMode::Edit;
                    break 'menu_running;
                }
            }
        }
        GameMode::Prelude => {
            let (tx, rx) = mpsc::channel();

            let handle = thread::spawn(move || -> WeeResult<()> {
                let games_list = GamesList::new()?;

                let filename = games_list.choose();

                let game_data = GameData::load(&filename)?;

                tx.send((games_list, game_data, filename))?;

                Ok(())
            });
            match handle.join().unwrap() {
                Ok(()) => {
                    let completion = LoadedGame::load("games/system/prelude.json", &intro_font)?
                        .start(DEFAULT_GAME_SPEED, config.settings())
                        .play(renderer, event_pump)?;

                    match rx.recv() {
                        Ok((list, game_data, filename)) => {
                            let games_list = list;

                            let game =
                                LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

                            let progress = Progress::new(config.playback_rate.min);

                            log::info!("{:?}", games_list);

                            game_mode = if let Completion::Finished = completion {
                                GameMode::Play {
                                    game,
                                    games_list,
                                    progress,
                                }
                            } else {
                                GameMode::Menu
                            }
                        }
                        Err(error) => {
                            game_mode = GameMode::Error(Box::new(error));
                        }
                    }
                }
                Err(error) => {
                    game_mode = GameMode::Error(error);
                }
            }
        }
        GameMode::Interlude {
            won,
            games_list,
            progress,
        } => {
            let (tx, rx) = mpsc::channel();

            let filename = games_list.choose();
            thread::spawn(move || -> WeeResult<()> {
                let new_game = {
                    let game_data = GameData::load(&filename.clone())?;
                    (filename, game_data)
                };

                tx.send(new_game)?;

                Ok(())
            });

            let mut loaded_game = LoadedGame::load("games/system/interlude.json", &intro_font)?;

            for object in loaded_game.objects.iter_mut() {
                let mut set_switch = |name, pred| {
                    if object.name == name {
                        object.switch = if pred { Switch::On } else { Switch::Off };
                    }
                };
                set_switch("Won", won);
                set_switch("1", progress.lives >= 1);
                set_switch("2", progress.lives >= 2);
                set_switch("3", progress.lives >= 3);
                set_switch("4", progress.lives >= 4);

                replace_text(object, &progress);
            }

            let completion = loaded_game
                .start(progress.playback_rate, config.settings())
                .play(renderer, event_pump)?;

            let (filename, game_data) = rx.recv()?;

            let game = LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

            log::info!("Playing game: {}", filename);
            game_mode = if let Completion::Finished = completion {
                GameMode::Play {
                    game,
                    games_list,
                    progress,
                }
            } else {
                GameMode::Menu
            };
        }
        GameMode::GameOver { progress } => {
            let mut loaded_game = LoadedGame::load("games/system/game-over.json", &intro_font)?;
            for object in loaded_game.objects.iter_mut() {
                replace_text(object, &progress);
            }
            loaded_game
                .start(DEFAULT_GAME_SPEED, config.settings())
                .play(renderer, event_pump)?;

            game_mode = GameMode::Menu;
        }
        GameMode::Play {
            game: loaded_game,
            games_list,
            mut progress,
        } => {
            let mut game = loaded_game.start(progress.playback_rate, config.settings());

            let result = game.play(renderer, event_pump);

            match result {
                Ok(completion) => {
                    game_mode = if let Completion::Finished = completion {
                        let has_won = game.has_been_won();

                        progress.update(has_won, &config.playback_rate);
                        log::info!("Playback Rate: {}", progress.playback_rate);

                        if progress.lives == 0 {
                            GameMode::GameOver { progress }
                        } else {
                            GameMode::Interlude {
                                won: has_won,
                                games_list,
                                progress,
                            }
                        }
                    } else {
                        GameMode::Menu
                    }
                }
                Err(error) => {
                    log::error!("{}", error);
                    game_mode = GameMode::Error(error);
                }
            }
        }
        GameMode::Edit => {
            let mut last_frame = Instant::now();
            let mut return_to_menu = false;
            'editor_running: loop {
                if sdlglue::has_quit(event_pump) {
                    process::exit(0);
                }
                sdlglue::set_fullscreen(renderer, event_pump)?;

                let imgui_frame = imgui.prepare_frame(&renderer, &event_pump, &mut last_frame);
                let ui = &imgui_frame.ui;

                ui.show_demo_window(&mut true);

                let menu_bar = ui.begin_main_menu_bar();
                if let Some(bar) = menu_bar {
                    let menu = ui.begin_menu(im_str!("File"), true);
                    if let Some(menu) = menu {
                        if imgui::MenuItem::new(im_str!("Return to Menu")).build(ui) {
                            game_mode = GameMode::Menu;
                            return_to_menu = true;
                        }
                        menu.end(ui);
                    }
                    bar.end(ui);
                }

                sdlglue::clear_screen(Colour::dull_grey());
                imgui_frame.render(&renderer);
                renderer.present();

                thread::sleep(Duration::from_millis(10));

                if return_to_menu {
                    break 'editor_running;
                }
            }
        }
        GameMode::Error(error) => {
            let mut last_frame = Instant::now();
            let mut do_break = false;
            'error_running: loop {
                if sdlglue::has_quit(event_pump) {
                    process::exit(0);
                }
                sdlglue::set_fullscreen(renderer, event_pump)?;
                sdlglue::clear_screen(Colour::dull_grey());

                let imgui_frame = imgui.prepare_frame(&renderer, &event_pump, &mut last_frame);
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

                imgui_frame.render(&renderer);

                renderer.present();
            }

            game_mode = GameMode::Menu;
        }
    }
    Ok(game_mode)
}

fn main() -> WeeResult<()> {
    init_logger();

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

    let _gl_context = GLContext::from_sdl(&video_subsystem, &window);

    gl::load_with(|s| video_subsystem.gl_get_proc_address(s) as _);

    unsafe {
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    }

    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).show_error()?;

    let config = Config::from_file("config.yaml")
        .map_err(|error| format!("Error loading configuration from config.yaml.\n{}", error))
        .show_error()?;

    let mut imgui = Imgui::init(&config, &video_subsystem, &window).show_error()?;

    let intro_font = IntroFont::load(&config.intro_font, &ttf_context).show_error()?;

    let mut event_pump = sdl_context.event_pump().show_error()?;

    let mut renderer = Renderer::new(window);

    let mut game_mode = GameMode::Menu;

    loop {
        let loop_result = run_main_loop(
            game_mode,
            &mut renderer,
            &mut event_pump,
            &mut imgui,
            &intro_font,
            &config,
        );
        game_mode = match loop_result {
            Ok(game_mode) => game_mode,
            Err(error) => GameMode::Error(error),
        };
    }
}
