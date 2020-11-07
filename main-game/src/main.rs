#![windows_subsystem = "windows"]

#[macro_use]
extern crate imgui;

use rand::{seq::SliceRandom, thread_rng, Rng};
use sdl2::{
    image::InitFlag,
    messagebox::{self, MessageBoxFlag},
    video::{gl_attr::GLAttr, GLContext, Window as SdlWindow},
    Sdl, VideoSubsystem,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{error::Error, fs, path::Path, process, str, time::Instant};
use walkdir::WalkDir;

use editor::*;
use sdlglue::Renderer;
use wee::*;
use wee_common::{Colour, WeeResult};

const BOSS_GAME_INTERVAL: i32 = 15;

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

enum GameMode<'a, 'b> {
    Menu,
    Play {
        game: LoadedGame<'a, 'b>,
        games_list: GamesList,
        progress: Progress,
    },
    Edit,
    ChooseMode,
    Prelude {
        directory: Option<String>,
    },
    Interlude {
        won: bool,
        games_list: GamesList,
        progress: Progress,
    },
    GameOver {
        progress: Progress,
        directory: String,
    },
    Error(Box<dyn Error + Send + Sync>),
}

#[derive(Debug, Copy, Clone)]
struct Progress {
    playback_rate: f32,
    score: i32,
    lives: i32,
    difficulty: u32,
}

const MAX_LIVES: i32 = 4;
const UP_TO_DIFFICULTY_TWO: i32 = 20;
const UP_TO_DIFFICULTY_THREE: i32 = 40;

impl Progress {
    fn new(playback_rate: f32) -> Progress {
        Progress {
            playback_rate,
            score: 0,
            lives: MAX_LIVES,
            difficulty: DEFAULT_DIFFICULTY,
        }
    }

    fn update(&mut self, has_won: bool, playback_increase: f32, playback_max: f32) {
        if has_won {
            self.score += 1;
            if self.score % 5 == 0 {
                self.playback_rate += playback_increase;
            }
            if self.score >= UP_TO_DIFFICULTY_THREE {
                self.difficulty = 3;
            } else if self.score >= UP_TO_DIFFICULTY_TWO {
                self.difficulty = 2;
            }
            self.playback_rate = self.playback_rate.min(playback_max);
        } else {
            self.lives -= 1;
        }
    }
}

#[derive(Debug)]
struct GamesList {
    games: Vec<String>,
    bosses: Vec<String>,
    next_boss_game: i32,
    next: Vec<String>,
    directory: String,
}

impl GamesList {
    fn from_directory(directory: &str) -> WeeResult<GamesList> {
        let mut games_list = Vec::new();
        let mut boss_list = Vec::new();
        for entry in WalkDir::new(directory).into_iter().filter_map(|e| e.ok()) {
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
                            if data.game_type == GameType::Minigame {
                                games_list.push(filename.to_string());
                            } else if data.game_type == GameType::BossGame {
                                boss_list.push(filename.to_string());
                            }
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
            Ok(GamesList {
                games: games_list,
                bosses: boss_list,
                next_boss_game: BOSS_GAME_INTERVAL,
                next: Vec::new(),
                directory: if directory == "games" {
                    "games/system/".to_string()
                } else if directory.ends_with('/') {
                    directory.to_string()
                } else {
                    let mut directory = directory.to_string();
                    directory.push_str("/");
                    directory
                },
            })
        }
    }

    fn choose_boss(&self) -> Option<String> {
        self.bosses.choose(&mut thread_rng()).cloned()
    }

    fn choose(&mut self) -> String {
        assert!(!self.games.is_empty());
        if self.next.is_empty() {
            let mut games = self.games.clone();
            for _ in 0..15 {
                if games.is_empty() {
                    games = self.games.clone();
                }
                let game = games.remove(thread_rng().gen_range(0, games.len()));
                self.next.push(game);
            }
        }

        log::debug!("{:?}", self.next);

        self.next.remove(0)
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

fn run_main_loop<'a, 'b>(
    mut game_mode: GameMode<'a, 'b>,
    renderer: &mut Renderer,
    events: &mut EventState,
    imgui: &mut Imgui,
    intro_font: &'a FontSystem<'a, 'b>,
    config: &Config,
) -> WeeResult<GameMode<'a, 'b>> {
    let game_with_defaults = |filename| -> WeeResult<Game> {
        let game = LoadedGame::load(filename, &intro_font)
            .map_err(|error| format!("Couldn't load {}\n{}", filename, error))?
            .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings());
        Ok(game)
    };
    let mode_path = |directory: &str, filename| {
        let mut path = directory.to_string();
        if !path.ends_with('/') {
            path.push_str("/");
        };
        path.push_str(filename);
        path
    };
    let mode_game = |directory, filename| -> WeeResult<LoadedGame> {
        let game_path = mode_path(directory, filename);

        let game = LoadedGame::load(game_path, &intro_font);
        if let Ok(game) = game {
            Ok(game)
        } else {
            LoadedGame::load(format!("games/system/{}", filename), &intro_font)
        }
    };
    match game_mode {
        GameMode::Menu => {
            let mut game = game_with_defaults("games/system/main-menu.json")?;

            'menu_running: loop {
                renderer.adjust_fullscreen(&events.pump, &events.mouse.utils)?;

                game.update_and_render_frame(renderer, events)?;

                if wee::is_switched_on(&game.objects, "Play") {
                    game_mode = GameMode::ChooseMode;
                    break 'menu_running;
                }
                if wee::is_switched_on(&game.objects, "Edit") {
                    game_mode = GameMode::Edit;
                    break 'menu_running;
                }
                if wee::is_switched_on(&game.objects, "Quit") {
                    process::exit(0);
                }
            }
        }
        GameMode::ChooseMode => {
            let mut game = game_with_defaults("games/system/choose-mode.json")?;

            'choose_mode_running: loop {
                renderer.adjust_fullscreen(&events.pump, &events.mouse.utils)?;

                game.update_and_render_frame(renderer, events)?;

                for (key, object) in game.objects.iter() {
                    if object.switch == SwitchState::SwitchedOn {
                        let pattern = "OpenFolder:";
                        if key.starts_with(pattern) {
                            let directory = key[pattern.len()..].to_string();
                            game_mode = GameMode::Prelude {
                                directory: Some(directory),
                            };
                            break 'choose_mode_running;
                        }
                        if key == "Shuffle" {
                            game_mode = GameMode::Prelude { directory: None };
                            break 'choose_mode_running;
                        }
                        if key == "Back" {
                            game_mode = GameMode::Menu;
                            break 'choose_mode_running;
                        }
                    }
                }
            }
        }
        GameMode::Prelude { directory } => {
            let (tx, rx) = std::sync::mpsc::channel();

            let directory = directory.unwrap_or_else(|| "games".to_string());
            let prelude_game = mode_game(&directory, "prelude.json")?;

            std::thread::spawn(move || -> WeeResult<()> {
                let games_list = GamesList::from_directory(&directory)?;

                tx.send(games_list)?;

                Ok(())
            });

            let completed_game = prelude_game
                .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
                .play(renderer, events)?;

            let mut games_list: GamesList = rx.recv()?;

            let filename = games_list.choose();

            let game_data = GameData::load(&filename)?;

            let game = LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

            let progress = Progress::new(config.playback_rate.min);

            log::info!("{:?}", games_list);

            game_mode = if let Completion::Finished = completed_game.completion {
                GameMode::Play {
                    game,
                    games_list,
                    progress,
                }
            } else {
                GameMode::ChooseMode
            };
        }
        GameMode::Interlude {
            won,
            mut games_list,
            progress,
        } => {
            let is_boss_game =
                !games_list.bosses.is_empty() && games_list.next_boss_game == progress.score;
            let filename = if is_boss_game {
                games_list.choose_boss().unwrap()
            } else {
                games_list.choose()
            };

            let game_data = GameData::load(&filename.clone())?;

            let mut loaded_game = mode_game(&games_list.directory, "interlude.json")?;

            let text_replacements = vec![
                ("{Score}", progress.score.to_string()),
                ("{Lives}", progress.lives.to_string()),
                (
                    "{Game}",
                    Path::new(&filename)
                        .file_stem()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string(),
                ),
                (
                    "{IntroText}",
                    game_data.intro_text.as_deref().unwrap_or("").to_string(),
                ),
            ];
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
                set_switch("Boss", games_list.next_boss_game == progress.score);

                object.replace_text(&text_replacements);
            }

            let completed_game = loaded_game
                .start(
                    progress.playback_rate,
                    DEFAULT_DIFFICULTY,
                    config.settings(),
                )
                .play(renderer, events)?;

            let game = LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

            log::info!("Playing game: {}", filename);
            game_mode = if let Completion::Finished = completed_game.completion {
                GameMode::Play {
                    game,
                    games_list,
                    progress,
                }
            } else {
                GameMode::ChooseMode
            };
        }
        GameMode::GameOver {
            progress,
            directory,
        } => {
            let path = Path::new(&directory).join("high-scores.yaml");
            let mut high_scores: (i32, i32, i32) = {
                log::info!("path: {:?}", path);
                let yaml = fs::read_to_string(&path);
                if let Ok(yaml) = yaml {
                    yaml_from_str(&yaml)?
                } else {
                    (0, 0, 0)
                }
            };

            let high_score_position = if progress.score >= high_scores.0 {
                high_scores.2 = high_scores.1;
                high_scores.1 = high_scores.0;
                high_scores.0 = progress.score;
                Some(1)
            } else if progress.score >= high_scores.1 {
                high_scores.2 = high_scores.1;
                high_scores.1 = progress.score;
                Some(2)
            } else if progress.score >= high_scores.2 {
                high_scores.2 = progress.score;
                Some(3)
            } else {
                None
            };

            {
                let s = serde_yaml::to_string(&high_scores)?;
                std::fs::write(&path, s).unwrap_or_else(|e| log::error!("{}", e));
            }

            let mut loaded_game = mode_game(&directory, "game-over.json")?;

            let text_replacements = vec![
                ("{Score}", progress.score.to_string()),
                ("{Lives}", progress.lives.to_string()),
                ("{1st}", high_scores.0.to_string()),
                ("{2nd}", high_scores.1.to_string()),
                ("{3rd}", high_scores.2.to_string()),
            ];
            for object in loaded_game.objects.iter_mut() {
                object.replace_text(&text_replacements);

                let mut set_switch = |name, pred| {
                    if object.name == name {
                        object.switch = if pred { Switch::On } else { Switch::Off };
                    }
                };
                set_switch("1st", high_score_position == Some(1));
                set_switch("2nd", high_score_position == Some(2));
                set_switch("3rd", high_score_position == Some(3));
            }
            loaded_game
                .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
                .play(renderer, events)?;

            game_mode = GameMode::ChooseMode;
        }
        GameMode::Play {
            game: loaded_game,
            mut games_list,
            mut progress,
        } => {
            let result = loaded_game
                .start(
                    progress.playback_rate,
                    progress.difficulty,
                    config.settings(),
                )
                .play(renderer, events);

            match result {
                Ok(completed_game) => {
                    game_mode = if let Completion::Finished = completed_game.completion {
                        let has_won = completed_game.has_been_won;
                        let was_boss_game = !games_list.bosses.is_empty()
                            && games_list.next_boss_game == progress.score;

                        progress.update(
                            has_won,
                            config.playback_rate.increase,
                            config.playback_rate.max,
                        );
                        if has_won && was_boss_game {
                            progress.lives = (progress.lives + 1).min(MAX_LIVES);
                        }
                        log::info!("Playback Rate: {}", progress.playback_rate);

                        if progress.lives <= 0 {
                            GameMode::GameOver {
                                progress,
                                directory: games_list.directory,
                            }
                        } else {
                            if was_boss_game {
                                games_list.next_boss_game += BOSS_GAME_INTERVAL;
                            }

                            GameMode::Interlude {
                                won: has_won,
                                games_list,
                                progress,
                            }
                        }
                    } else {
                        GameMode::ChooseMode
                    }
                }
                Err(error) => {
                    log::error!("{}", error);
                    game_mode = GameMode::Error(error);
                }
            }
        }
        GameMode::Edit => {
            let was_fullscreen =
                if let sdl2::video::FullscreenType::Off = renderer.window.fullscreen_state() {
                    false
                } else {
                    true
                };
            renderer.exit_fullscreen(&events.mouse.utils)?;
            events.mouse.utils.set_relative_mouse_mode(false);
            editor::run(renderer, events, imgui, intro_font, config.settings())?;
            if was_fullscreen {
                renderer.enter_fullscreen(&events.mouse.utils)?;
            }
            game_mode = GameMode::Menu;
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

            game_mode = GameMode::Menu;
        }
    }
    Ok(game_mode)
}

struct MainGame<'a, 'b> {
    game_mode: GameMode<'a, 'b>,
    renderer: Renderer,
    events: EventState,
    imgui: Imgui,
    config: Config,
}

impl<'a, 'b> MainGame<'a, 'b> {
    fn init(
        config: Config,
        sdl_context: &Sdl,
        video_subsystem: &VideoSubsystem,
        window: SdlWindow,
    ) -> WeeResult<MainGame<'a, 'b>> {
        let imgui = Imgui::init(&config.ui_font, &video_subsystem, &window)?;

        let event_pump = sdl_context.event_pump()?;
        let events = EventState {
            pump: event_pump,
            mouse: MouseState::new(config.sensitivity, sdl_context.mouse()),
        };

        let mouse_texture =
            sdlglue::Texture::from_file_with_filtering("games/system/images/mouse.png")?;

        let renderer = Renderer::new(window, mouse_texture);

        let game_mode = GameMode::Menu;

        Ok(MainGame {
            game_mode,
            renderer,
            events,
            imgui,
            config,
        })
    }

    fn run(mut self, font_system: &'a FontSystem<'a, 'b>) {
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
            .fullscreen()
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
