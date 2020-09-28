// TODO: Show origin debug option
// TODO: Exit fullscreen when in editor
// TODO: Aspect ratio of edited game changes when window size changed
// TODO: Holding in mouse button between main menu and choose mode causes it to flip

//#![windows_subsystem = "windows"]

#[macro_use]
extern crate imgui;

use rand::{seq::SliceRandom, thread_rng, Rng};
use sdl2::{
    image::InitFlag,
    messagebox::{self, MessageBoxFlag},
    video::{gl_attr::GLAttr, GLContext, Window as SdlWindow},
    EventPump,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{error::Error, fs, path::Path, process, str, sync::mpsc, thread, time::Instant};
use walkdir::WalkDir;

use sdlglue::Renderer;
/*use wee::{
    Action, Angle, AngleSetter, AnimationStatus, AnimationType, AssetFiles, Assets,
    BounceDirection, ButtonState, CollisionWith, CompassDirection, Completion, DrawIntroText,
    Effect, FontLoadInfo, GameData, GameSettings, ImageList, Images, Input, IntroFont,
    IntroFontConfig, IntroText, JumpLocation, LoadImages, LoadedGame, Motion, MouseInteraction,
    MouseOver, MovementDirection, MovementHandling, MovementType, PropertyCheck, PropertySetter,
    RelativeTo, RenderScene, SerialiseObject, SerialiseObjectList, Speed, Sprite,
    Switch, SwitchState, Target, TargetType, TextResize, Trigger, When, WinStatus,
    DEFAULT_GAME_SPEED,
};*/
use editor::*;
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
const UP_TO_DIFFICULTY_TWO: i32 = 15;
const UP_TO_DIFFICULTY_THREE: i32 = 30;

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
    next: Vec<String>,
    directory: String,
}

impl GamesList {
    fn from_directory(directory: Option<&str>) -> WeeResult<GamesList> {
        let mut games_list = Vec::new();
        let directory = directory.unwrap_or("games");
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
            Ok(GamesList {
                games: games_list,
                next: Vec::new(),
                directory: if directory == "games" {
                    "games/system/".to_string()
                } else {
                    if directory.ends_with("/") {
                        directory.to_string()
                    } else {
                        let mut directory = directory.to_string();
                        directory.push_str("/");
                        directory
                    }
                },
            })
        }
    }

    // TODO: Tidy up
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

        /*self.games
        .choose(&mut thread_rng())
        .expect("Games list is empty")
        .to_string()*/
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
                    .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
            };

            'menu_running: loop {
                sdlglue::set_fullscreen(renderer, event_pump)?;

                game.update_and_render_frame(renderer, event_pump)?;

                if wee::is_switched_on(&game.objects, "Play") {
                    game_mode = GameMode::ChooseMode;
                    break 'menu_running;
                }
                if wee::is_switched_on(&game.objects, "Edit") {
                    game_mode = GameMode::Edit;
                    break 'menu_running;
                }
            }
        }
        GameMode::ChooseMode => {
            let mut game = {
                let filename = "games/system/choose-mode.json";

                LoadedGame::load(filename, &intro_font)
                    .map_err(|error| format!("Couldn't load {}\n{}", filename, error))?
                    .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
            };

            'choose_mode_running: loop {
                sdlglue::set_fullscreen(renderer, event_pump)?;

                game.update_and_render_frame(renderer, event_pump)?;

                for (key, object) in game.objects.iter() {
                    if object.switch == SwitchState::SwitchedOn {
                        let pattern = "OpenFolder:";
                        if key.starts_with(pattern) {
                            let directory = key[pattern.len()..].to_string();
                            game_mode = GameMode::Prelude {
                                directory: Some(directory.to_string()),
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
            let (tx, rx) = mpsc::channel();

            let handle = thread::spawn(move || -> WeeResult<()> {
                let mut games_list = GamesList::from_directory(directory.as_deref())?;

                let filename = games_list.choose();

                let game_data = GameData::load(&filename)?;

                tx.send((games_list, game_data, filename))?;

                Ok(())
            });
            // TODO: Not in parallel
            match handle.join().unwrap() {
                Ok(()) => match rx.recv() {
                    Ok((games_list, game_data, filename)) => {
                        let interlude_path = {
                            let filename = if games_list.directory.ends_with("/") {
                                "prelude.json"
                            } else {
                                "/prelude.json"
                            };
                            let mut s = games_list.directory.clone();
                            s.push_str(filename);
                            s
                        };

                        let loaded_game = {
                            let game = LoadedGame::load(&interlude_path, &intro_font);
                            if let Ok(game) = game {
                                game
                            } else {
                                LoadedGame::load("games/system/prelude.json", &intro_font)?
                            }
                        };

                        let completed_game = loaded_game
                            .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
                            .play(renderer, event_pump)?;

                        let game =
                            LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

                        let progress = Progress::new(config.playback_rate.min);

                        log::info!("{:?}", games_list);

                        game_mode = if let Completion::Finished = completed_game.completion {
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
                },
                Err(error) => {
                    game_mode = GameMode::Error(error);
                }
            }
        }
        GameMode::Interlude {
            won,
            mut games_list,
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

            // TODO: This happens right after, not in parallel
            let (filename, game_data) = rx.recv()?;

            let interlude_path = {
                let filename = if games_list.directory.ends_with("/") {
                    "interlude.json"
                } else {
                    "/interlude.json"
                };
                let mut s = games_list.directory.clone();
                s.push_str(filename);
                s
            };

            let mut loaded_game = {
                let game = LoadedGame::load(&interlude_path, &intro_font);
                if let Ok(game) = game {
                    game
                } else {
                    LoadedGame::load("games/system/interlude.json", &intro_font)?
                }
            };

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

                object.replace_text(progress.score, progress.lives);

                // TODO: Tidy up

                for instruction in object.instructions.iter_mut() {
                    fn replace_text_in_action(
                        action: &mut Action,
                        score: i32,
                        lives: i32,
                        filename: &str,
                        intro_text: &str,
                    ) {
                        if let Action::DrawText { text, .. } = action {
                            let path = Path::new(filename);
                            // TODO: Have name field in game instead of filename for this
                            *text =
                                text.replace("{Game}", path.file_stem().unwrap().to_str().unwrap());
                            *text = text.replace("{IntroText}", intro_text);
                        } else if let Action::Random { random_actions } = action {
                            for action in random_actions {
                                replace_text_in_action(action, score, lives, filename, intro_text);
                            }
                        }
                    }

                    for action in instruction.actions.iter_mut() {
                        replace_text_in_action(
                            action,
                            progress.score,
                            progress.lives,
                            &filename,
                            game_data.intro_text.as_deref().unwrap_or(""),
                        );
                    }
                }
            }

            let completed_game = loaded_game
                .start(
                    progress.playback_rate,
                    DEFAULT_DIFFICULTY,
                    config.settings(),
                )
                .play(renderer, event_pump)?;

            let game = LoadedGame::load_from_game_data(game_data, &filename, &intro_font)?;

            log::info!("Playing game: {}", filename);
            game_mode = if let Completion::Finished = completed_game.completion {
                GameMode::Play {
                    game,
                    games_list,
                    progress,
                }
            } else {
                GameMode::Menu
            };
        }
        GameMode::GameOver {
            progress,
            directory,
        } => {
            let high_scores: (i32, i32, i32) = {
                let path = Path::new(&directory).join("high-scores.yaml");
                log::info!("path: {:?}", path);
                let yaml = fs::read_to_string(&path);
                let mut high_scores = if let Ok(yaml) = yaml {
                    yaml_from_str(&yaml)?
                } else {
                    (0, 0, 0)
                };

                if progress.score >= high_scores.0 {
                    high_scores.2 = high_scores.1;
                    high_scores.1 = high_scores.0;
                    high_scores.0 = progress.score;
                } else if progress.score >= high_scores.1 {
                    high_scores.2 = high_scores.1;
                    high_scores.1 = progress.score;
                } else if progress.score >= high_scores.2 {
                    high_scores.2 = progress.score;
                }

                let s = serde_yaml::to_string(&high_scores)?;
                std::fs::write(&path, s).unwrap_or_else(|e| log::error!("{}", e));

                high_scores
            };

            //let mut loaded_game = LoadedGame::load("games/system/game-over.json", &intro_font)?;
            let game_over_path = {
                let filename = if directory.ends_with("/") {
                    "game-over.json"
                } else {
                    "/game-over.json"
                };
                let mut s = directory.clone();
                s.push_str(filename);
                s
            };

            let mut loaded_game = {
                let game = LoadedGame::load(&game_over_path, &intro_font);
                if let Ok(game) = game {
                    game
                } else {
                    LoadedGame::load("games/system/game-over.json", &intro_font)?
                }
            };
            for object in loaded_game.objects.iter_mut() {
                object.replace_text(progress.score, progress.lives);

                // TODO: Tidy up

                for instruction in object.instructions.iter_mut() {
                    fn replace_text_in_action(action: &mut Action, high_scores: (i32, i32, i32)) {
                        if let Action::DrawText { text, .. } = action {
                            // TODO: Have name field in game instead of filename for this
                            *text = text.replace("{HighScore-1}", &high_scores.0.to_string());
                            *text = text.replace("{HighScore-2}", &high_scores.1.to_string());
                            *text = text.replace("{HighScore-3}", &high_scores.2.to_string());
                        } else if let Action::Random { random_actions } = action {
                            for action in random_actions {
                                replace_text_in_action(action, high_scores);
                            }
                        }
                    }

                    for action in instruction.actions.iter_mut() {
                        replace_text_in_action(action, high_scores);
                    }
                }
            }
            loaded_game
                .start(DEFAULT_GAME_SPEED, DEFAULT_DIFFICULTY, config.settings())
                .play(renderer, event_pump)?;

            game_mode = GameMode::Menu;
        }
        GameMode::Play {
            game: loaded_game,
            games_list,
            mut progress,
        } => {
            let result = loaded_game
                .start(
                    progress.playback_rate,
                    progress.difficulty,
                    config.settings(),
                )
                .play(renderer, event_pump);

            match result {
                Ok(completed_game) => {
                    game_mode = if let Completion::Finished = completed_game.completion {
                        let has_won = completed_game.has_been_won;

                        progress.update(
                            has_won,
                            config.playback_rate.increase,
                            config.playback_rate.max,
                        );
                        log::info!("Playback Rate: {}", progress.playback_rate);

                        if progress.lives == 0 {
                            GameMode::GameOver {
                                progress,
                                directory: games_list.directory,
                            }
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
            editor::run_editor(renderer, event_pump, imgui, intro_font, config.settings())?;
            game_mode = GameMode::Menu;
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

                let imgui_frame = imgui.prepare_frame(
                    &renderer.window,
                    &event_pump.mouse_state(),
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

    let _gl_context = GLContext::from_sdl(&video_subsystem.gl_attr(), &window);

    gl::load_with(|s| video_subsystem.gl_get_proc_address(s) as _);

    unsafe {
        gl::Enable(gl::BLEND);
        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
    }

    let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string()).show_error()?;

    let config = Config::from_file("config.yaml")
        .map_err(|error| format!("Error loading configuration from config.yaml.\n{}", error))
        .show_error()?;

    let mut imgui = Imgui::init(&config.ui_font, &video_subsystem, &window).show_error()?;

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
