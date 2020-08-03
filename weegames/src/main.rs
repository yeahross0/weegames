// TODO: Show origin debug option
// TODO: Enable selecting object again
// TODO: Exit fullscreen when in editor
// TODO: Fix crashes when switching between objects

//#![windows_subsystem = "windows"]

#[macro_use]
extern crate imgui;

use imgui::ImString;
use imgui_opengl_renderer::Renderer as ImguiRenderer;
use imgui_sdl2::ImguiSdl2 as ImguiSdl;
use nfd::Response;
use rand::{seq::SliceRandom, thread_rng};
use sdl2::{
    event::Event,
    image::InitFlag,
    keyboard::Scancode,
    messagebox::{self, MessageBoxFlag},
    video::{gl_attr::GLAttr, GLContext, Window as SdlWindow},
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

use sdlglue::{Model, Renderer};
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
use wee_common::{
    Colour, Flip, Rect, Vec2, WeeResult, PROJECTION_HEIGHT, PROJECTION_WIDTH,
};

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
        ui_font: &FontLoadInfo,
        video_subsystem: &VideoSubsystem,
        window: &SdlWindow,
    ) -> WeeResult<Imgui> {
        let mut context = {
            let mut context = imgui::Context::create();
            context.set_ini_filename(None);

            context.fonts().clear();

            let bytes = std::fs::read(&ui_font.filename)?;

            context.fonts().add_font(&[imgui::FontSource::TtfData {
                data: &bytes,
                size_pixels: ui_font.size as f32,
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
        window: &SdlWindow,
        mouse_state: &sdl2::mouse::MouseState,
        last_frame: &mut Instant,
    ) -> ImguiFrame {
        self.sdl
            .prepare_frame(self.context.io_mut(), window, mouse_state);

        self.context.io_mut().delta_time = Self::new_time(last_frame);

        ImguiFrame {
            ui: self.context.frame(),
            sdl: &mut self.sdl,
            renderer: &mut self.renderer,
        }
    }

    fn new_time(last_frame: &mut Instant) -> f32 {
        let imgui_now = Instant::now();
        let delta = imgui_now - *last_frame;
        let delta_s = delta.as_secs() as f32 + delta.subsec_nanos() as f32 / 1_000_000_000.0;
        *last_frame = imgui_now;
        delta_s
    }
}

struct ImguiFrame<'a> {
    ui: imgui::Ui<'a>,
    sdl: &'a mut ImguiSdl,
    renderer: &'a mut ImguiRenderer,
}

impl<'a> ImguiFrame<'a> {
    fn render(self, window: &SdlWindow) {
        self.sdl.prepare_render(&self.ui, window);
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

    fn update(&mut self, has_won: bool, playback_increase: f32, playback_max: f32) {
        if has_won {
            self.score += 1;
            if self.score % 5 == 0 {
                self.playback_rate += playback_increase;
            }
            self.playback_rate = self.playback_rate.min(playback_max);
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

                object.replace_text(progress.score, progress.lives);
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
                object.replace_text(progress.score, progress.lives);
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

                        progress.update(
                            has_won,
                            config.playback_rate.increase,
                            config.playback_rate.max,
                        );
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
            let mut game = GameData::default();
            let mut images = Images::new();
            let mut filename = None;
            let mut game_position = Vec2::zero();
            let mut scale: f32 = 1.0;
            let mut minus_button = ButtonState::Up;
            let mut plus_button = ButtonState::Up;
            let mut show_collision_areas = false;
            let mut selected_index = None;
            let mut instruction_index = 0;
            let mut instruction_mode = InstructionMode::View;
            let mut instruction_focus = InstructionFocus::None;
            struct RenameObject {
                index: usize,
                name: String,
                buffer: ImString,
            }
            let mut rename_object: Option<RenameObject> = None;
            'editor_running: loop {
                for event in event_pump.poll_iter() {
                    imgui.sdl.handle_event(&mut imgui.context, &event);
                    if imgui.sdl.ignore_event(&event) {
                        continue;
                    }
                    if let Event::Quit { .. } = event {
                        process::exit(0);
                    }
                }
                sdlglue::set_fullscreen(renderer, event_pump)?;

                let imgui_frame = imgui.prepare_frame(
                    &renderer.window,
                    &event_pump.mouse_state(),
                    &mut last_frame,
                );
                let ui = &imgui_frame.ui;

                ui.show_demo_window(&mut true);

                let menu_bar = ui.begin_main_menu_bar();
                if let Some(bar) = menu_bar {
                    let menu = ui.begin_menu(im_str!("File"), true);
                    if let Some(menu) = menu {
                        if imgui::MenuItem::new(im_str!("Open")).build(&ui) {
                            let games_path =
                                std::env::current_dir().unwrap().join(Path::new("games"));
                            let response = nfd::open_file_dialog(None, games_path.to_str());
                            if let Ok(response) = response {
                                for _ in event_pump.poll_iter() {}
                                match response {
                                    Response::Okay(file_path) => {
                                        log::info!("File path = {:?}", file_path);
                                        let new_data = GameData::load(&file_path);
                                        match new_data {
                                            Ok(new_data) => {
                                                game = new_data;
                                                let base_path =
                                                    Path::new(&file_path).parent().unwrap();
                                                images = Images::load(
                                                    &game.asset_files.images,
                                                    &base_path,
                                                )?;
                                                filename = Some(file_path);
                                            }
                                            Err(error) => {
                                                log::error!("Couldn't open file {}", file_path);
                                                log::error!("{}", error);
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
                                log::error!("Error opening file dialog");
                                //ui.open_popup(im_str!("Error Opening File"));
                            }
                        }
                        if imgui::MenuItem::new(im_str!("Return to Menu")).build(ui) {
                            game_mode = GameMode::Menu;
                            return_to_menu = true;
                        }
                        menu.end(ui);
                    }

                    let menu = ui.begin_menu(im_str!("Debug"), true);
                    if let Some(menu) = menu {
                        let toggle = |label, opened: &mut bool| {
                            let mut toggled = *opened;
                            if imgui::MenuItem::new(label).build_with_ref(ui, &mut toggled) {
                                *opened = !(*opened);
                            }
                        };

                        toggle(im_str!("Show Collision Areas"), &mut show_collision_areas);
                        menu.end(ui);
                    }
                    bar.end(ui);
                }
                let window_size = [500.0, 600.0];

                imgui::Window::new(im_str!("Main Window"))
                    .size(window_size, imgui::Condition::FirstUseEver)
                    .scroll_bar(true)
                    .scrollable(true)
                    .resizable(true)
                    .build(&ui, || {
                        if ui.button(im_str!("Play"), NORMAL_BUTTON) {
                            LoadedGame::load_from_game_data(
                                game.clone(),
                                filename.as_ref().unwrap(),
                                &intro_font,
                            )
                            .unwrap()
                            .start(1.0, config.settings())
                            .play(renderer, event_pump)
                            .unwrap();
                        }
                    });

                imgui::Window::new(im_str!("Objects"))
                    .size(window_size, imgui::Condition::FirstUseEver)
                    //.position([900.0, 200.0], imgui::Condition::FirstUseEver)
                    .scroll_bar(true)
                    .scrollable(true)
                    .resizable(true)
                    .build(&ui, || {
                        imgui::ChildWindow::new(im_str!("Left"))
                            .size([150.0, 0.0])
                            .border(true)
                            .horizontal_scrollbar(true)
                            .build(&ui, || {
                                #[derive(Debug)]
                                enum MoveDirection {
                                    Up,
                                    Down,
                                }
                                #[derive(Debug)]
                                enum ObjectOperation {
                                    Rename {
                                        index: usize,
                                        from: String,
                                        to: String,
                                    },
                                    Delete {
                                        index: usize,
                                    },
                                    Move {
                                        direction: MoveDirection,
                                        index: usize,
                                    },
                                    None,
                                }

                                let up_arrow = ui.key_index(imgui::Key::UpArrow);
                                let down_arrow = ui.key_index(imgui::Key::DownArrow);
                                let up_pressed = ui.is_key_pressed(up_arrow);
                                let down_pressed = ui.is_key_pressed(down_arrow);

                                let mut object_operation: ObjectOperation = ObjectOperation::None;

                                // TODO: Tidy up
                                let is_being_renamed =
                                    |rename_object: &Option<RenameObject>, index| {
                                        if let Some(rename_details) = rename_object {
                                            return rename_details.index == index;
                                        }
                                        return false;
                                    };

                                for i in 0..game.objects.len() {
                                    if is_being_renamed(&rename_object, i) {
                                        if let Some(rename_details) = &mut rename_object {
                                            if ui
                                                .input_text(
                                                    im_str!("##edit"),
                                                    &mut rename_details.buffer,
                                                )
                                                .enter_returns_true(true)
                                                .build()
                                            {
                                                object_operation = ObjectOperation::Rename {
                                                    index: rename_details.index,
                                                    from: rename_details.name.clone(),
                                                    to: rename_details.buffer.to_string(),
                                                };

                                                // TODO: try selected_object instead
                                                rename_object = None;
                                            } else if ui.is_item_deactivated() {
                                                // TODO: Not deactivating anymore
                                                object_operation = ObjectOperation::Rename {
                                                    index: rename_details.index,
                                                    from: rename_details.name.clone(),
                                                    to: rename_details.buffer.to_string(),
                                                };

                                                rename_object = None;
                                            }
                                        }
                                    } else {
                                        if imgui::Selectable::new(&im_str!(
                                            "{}",
                                            game.objects[i].name
                                        ))
                                        .selected(selected_index == Some(i))
                                        .build(&ui)
                                        {
                                            selected_index = Some(i);
                                        }
                                        if ui.is_item_active() {
                                            if up_pressed {
                                                object_operation = ObjectOperation::Move {
                                                    direction: MoveDirection::Up,
                                                    index: i,
                                                };
                                            } else if down_pressed {
                                                object_operation = ObjectOperation::Move {
                                                    direction: MoveDirection::Down,
                                                    index: i,
                                                };
                                            }
                                        }
                                        if ui.is_item_clicked(imgui::MouseButton::Right) {
                                            ui.open_popup(im_str!("Edit Object"));
                                            /*rename_object = Some(RenameObject {
                                                index: i,
                                                name: game.objects[i].name.clone(),
                                                buffer: ImString::from(
                                                    game.objects[i].name.clone(),
                                                ),
                                            });*/
                                            selected_index = Some(i);
                                        }
                                    }
                                }

                                ui.popup(im_str!("Edit Object"), || {
                                    if ui.button(im_str!("Move up"), NORMAL_BUTTON) {
                                        object_operation = ObjectOperation::Move {
                                            direction: MoveDirection::Up,
                                            index: selected_index.unwrap(),
                                        };
                                    }

                                    if ui.button(im_str!("Move down"), NORMAL_BUTTON) {
                                        object_operation = ObjectOperation::Move {
                                            direction: MoveDirection::Down,
                                            index: selected_index.unwrap(),
                                        };
                                    }

                                    if ui.button(im_str!("Delete"), NORMAL_BUTTON) {
                                        object_operation = ObjectOperation::Delete {
                                            index: selected_index.unwrap(),
                                        }
                                    }

                                    if ui.button(im_str!("Rename"), NORMAL_BUTTON) {
                                        rename_object = Some(RenameObject {
                                            index: selected_index.unwrap(),
                                            name: game.objects[selected_index.unwrap()]
                                                .name
                                                .clone(),
                                            buffer: ImString::from(
                                                game.objects[selected_index.unwrap()].name.clone(),
                                            ),
                                        });
                                        ui.close_current_popup();
                                    }

                                    /*if let Some(rename_details) = &mut rename_object {
                                        if ui
                                            .input_text(
                                                im_str!("Rename Object"),
                                                &mut rename_details.buffer,
                                            )
                                            .resize_buffer(true)
                                            .enter_returns_true(true)
                                            .build()
                                        {
                                            object_operation = ObjectOperation::Rename {
                                                index: rename_details.index,
                                                from: rename_details.name.clone(),
                                                to: rename_details.buffer.to_string(),
                                            };

                                            // TODO: try selected_object instead
                                            //rename_object = None;
                                            rename_details.name = rename_details.buffer.to_string();
                                        }
                                    }
                                    if ui.button(im_str!("Confirm"), NORMAL_BUTTON) {
                                        if let Some(rename_details) = &mut rename_object {
                                            object_operation = ObjectOperation::Rename {
                                                index: rename_details.index,
                                                from: rename_details.name.clone(),
                                                to: rename_details.buffer.to_string(),
                                            };
                                            rename_details.name = rename_details.buffer.to_string();
                                        }
                                    }*/
                                });

                                if !ui.is_any_mouse_down() && ui.is_window_focused() {
                                    if let Some(index) = &mut selected_index {
                                        if up_pressed {
                                            let previous_index =
                                                if *index == 0 { 0 } else { *index - 1 };
                                            game.objects.swap(*index, previous_index);
                                            *index = previous_index;
                                        } else if down_pressed {
                                            let next_index =
                                                (*index + 1).min(game.objects.len() - 1);
                                            game.objects.swap(*index, next_index);
                                            *index = next_index;
                                        }
                                    }
                                }
                                /*ui.popup(im_str!("Edit Object Name"), || {
                                    ui.text("Edit name:");
                                    let mut text = ImString::from("Test".to_string());
                                    ui.input_text(im_str!("##edit"), &mut text).build();
                                });*/

                                match object_operation {
                                    ObjectOperation::Rename { index, from, to } => {
                                        let duplicate = game.objects.iter().any(|o| o.name == to);
                                        if from == to {
                                            ui.close_current_popup();
                                        } else if duplicate {
                                            // TODO: Add this popup
                                            ui.open_popup(im_str!("Duplicate Name"));
                                        } else {
                                            // TODO: Remove unnecessary cloning
                                            game.objects[index].name = to.clone();
                                            rename_across_objects(&mut game.objects, &from, &to);
                                            ui.close_current_popup();
                                        }
                                    }
                                    ObjectOperation::Move { direction, index } => match direction {
                                        MoveDirection::Up => {
                                            if index > 0 {
                                                game.objects.swap(index - 1, index);
                                                selected_index = Some(index - 1);
                                            }
                                        }
                                        MoveDirection::Down => {
                                            if index + 1 < game.objects.len() {
                                                game.objects.swap(index, index + 1);
                                                selected_index = Some(index + 1);
                                            }
                                        }
                                    },
                                    ObjectOperation::Delete { index } => {
                                        game.objects.remove(index);
                                        if game.objects.is_empty() {
                                            selected_index = None;
                                        } else if index == 0 {
                                            selected_index = Some(0);
                                        } else {
                                            selected_index = Some(index - 1);
                                        }
                                    }
                                    _ => {}
                                }
                            });

                        ui.same_line(0.0);

                        if let Some(index) = selected_index {
                            let mut object = game.objects[index].clone();
                            let object_names: Vec<&str> =
                                game.objects.iter().map(|o| o.name.as_ref()).collect();
                            let sprites = game.objects.sprites();
                            right_window(
                                ui,
                                &mut object,
                                &object_names,
                                &sprites,
                                &mut game.asset_files,
                                game.length,
                                &mut images,
                                &filename,
                                &mut instruction_mode,
                                &mut instruction_index,
                                &mut instruction_focus,
                            );
                            game.objects[index] = object;
                        }
                    });

                // TODO: Fix up
                imgui::Window::new(im_str!("Background"))
                    .size(window_size, imgui::Condition::FirstUseEver)
                    //.opened(&mut opened)
                    .build(&ui, || {
                        for i in 0..game.background.len() {
                            let stack = ui.push_id(i as i32);
                            ui.text(im_str!("Layer {}", i));
                            if i != 0 {
                                ui.same_line(0.0);
                                if ui.small_button(im_str!("Move Up")) {
                                    game.background.swap(i, i - 1);
                                }
                            }
                            if i != game.background.len() - 1 {
                                ui.same_line(0.0);
                                if ui.small_button(im_str!("Move Down")) {
                                    game.background.swap(i, i + 1);
                                }
                            }
                            if ui.small_button(im_str!("Delete")) {
                                ui.same_line(0.0);
                                game.background.remove(i);
                                stack.pop(&ui);
                                break;
                            } else {
                                /*let mut choose_background_image = images
                                    .keys()
                                    .position(|k| *k == game.background[i].name)
                                    .unwrap_or(0);
                                if let Some(image_name) = build_image_name_changer_returns(
                                    &ui,
                                    game,
                                    images,
                                    &mut choose_background_image,
                                    &None,
                                ) {
                                    game.background[i].name = image_name.clone();
                                }*/

                                ui.input_float(
                                    im_str!("Min X"),
                                    &mut game.background[i].area.min.x,
                                )
                                .build();
                                ui.input_float(
                                    im_str!("Min Y"),
                                    &mut game.background[i].area.min.y,
                                )
                                .build();
                                ui.input_float(
                                    im_str!("Max X"),
                                    &mut game.background[i].area.max.x,
                                )
                                .build();
                                ui.input_float(
                                    im_str!("Max Y"),
                                    &mut game.background[i].area.max.y,
                                )
                                .build();
                            }

                            stack.pop(&ui);
                        }
                        if ui.button(im_str!("New Background"), NORMAL_BUTTON) {
                            if images.is_empty() {
                                ui.open_popup(im_str!("Add an image"));
                            } else {
                                /*let name = images.keys().next().unwrap().clone();
                                let (w, h) =
                                    (images[&name].width as f32, images[&name].height as f32);
                                game.background.push(Background {
                                    name,
                                    rect: Rect::new(800.0, 450.0, w, h),
                                });*/
                            }
                        };
                        ui.popup_modal(im_str!("Add an image")).build(|| {
                            if images.is_empty() {
                                // TODO: Replace this with button instead of combo
                                // Or maybe you don't need a button at all just go straight to it
                                /*let mut tmp = 0;
                                build_image_name_changer_returns(
                                    &ui, game, images, &mut tmp, &None,
                                );*/
                            } else {
                                /*game.background.push(BackgroundPart {
                                    name: images.keys().next().unwrap().clone(),
                                    rect: Rect::new(800.0, 450.0, 1600.0, 900.0),
                                });*/
                                ui.close_current_popup();
                            }
                            if ui.button(im_str!("Cancel"), NORMAL_BUTTON) {
                                ui.close_current_popup();
                            }
                        });
                    });

                let key_down = |event_pump: &EventPump, scancode: Scancode| {
                    event_pump.keyboard_state().is_scancode_pressed(scancode)
                };

                minus_button.update(key_down(event_pump, Scancode::Minus));
                plus_button.update(key_down(event_pump, Scancode::Equals));

                if !ui.io().want_capture_keyboard {
                    if minus_button == ButtonState::Press {
                        scale = (scale - 0.1).max(0.1);
                    }
                    if plus_button == ButtonState::Press {
                        scale = (scale + 0.1).min(4.0);
                    }

                    if event_pump.keyboard_state().is_scancode_pressed(Scancode::W) {
                        game_position.y += 2.0;
                    }
                    if event_pump.keyboard_state().is_scancode_pressed(Scancode::S) {
                        game_position.y -= 2.0;
                    }
                    if event_pump.keyboard_state().is_scancode_pressed(Scancode::A) {
                        game_position.x += 2.0;
                    }
                    if event_pump.keyboard_state().is_scancode_pressed(Scancode::D) {
                        game_position.x -= 2.0;
                    }
                }

                sdlglue::clear_screen(Colour::dull_grey());

                {
                    let dest = Rect::new(
                        (PROJECTION_WIDTH / 2.0 + game_position.x) * scale,
                        (PROJECTION_HEIGHT / 2.0 + game_position.y) * scale,
                        PROJECTION_WIDTH * scale,
                        PROJECTION_HEIGHT * scale,
                    );
                    let model = Model::new(dest, None, 0.0, Flip::default());
                    renderer.fill_rectangle(model, Colour::light_grey());
                }

                //renderer.draw_background(&game.background, &images)?;
                {
                    for part in game.background.iter() {
                        match &part.sprite {
                            Sprite::Image { name } => {
                                let texture = images.get_image(name)?;

                                let mut dest = part.area.to_rect();
                                dest.x += game_position.x;
                                dest.y += game_position.y;
                                dest.x *= scale;
                                dest.y *= scale;
                                dest.w *= scale;
                                dest.h *= scale;

                                renderer.prepare(&texture).set_dest(dest).draw();
                            }
                            Sprite::Colour(colour) => {
                                let mut dest = part.area.to_rect();
                                dest.x += game_position.x;
                                dest.y += game_position.y;
                                dest.x *= scale;
                                dest.y *= scale;
                                dest.w *= scale;
                                dest.h *= scale;

                                let model = Model::new(dest, None, 0.0, Flip::default());

                                renderer.fill_rectangle(model, *colour);
                            }
                        }
                    }
                }

                {
                    let mut layers: Vec<u8> = game.objects.iter().map(|o| o.layer).collect();
                    layers.sort();
                    layers.dedup();
                    layers.reverse();
                    for layer in layers.into_iter() {
                        for object in game.objects.iter() {
                            if object.layer == layer {
                                match &object.sprite {
                                    Sprite::Image { name: image_name } => {
                                        let texture = images.get_image(image_name)?;
                                        let mut dest = Rect::new(
                                            object.position.x + game_position.x,
                                            object.position.y + game_position.y,
                                            object.size.width,
                                            object.size.height,
                                        );

                                        dest.x *= scale;
                                        dest.y *= scale;
                                        dest.w *= scale;
                                        dest.h *= scale;

                                        let origin = object.origin() * scale;

                                        renderer
                                            .prepare(&texture)
                                            .set_dest(dest)
                                            .set_angle(object.angle)
                                            .set_origin(Some(origin))
                                            .flip(object.flip)
                                            .draw();
                                    }
                                    Sprite::Colour(colour) => {
                                        let mut dest = Rect::new(
                                            object.position.x + game_position.x,
                                            object.position.y + game_position.y,
                                            object.size.width,
                                            object.size.height,
                                        );

                                        dest.x *= scale;
                                        dest.y *= scale;
                                        dest.w *= scale;
                                        dest.h *= scale;

                                        let model = Model::new(
                                            dest,
                                            Some(object.origin() * scale),
                                            object.angle,
                                            object.flip,
                                        );

                                        renderer.fill_rectangle(model, *colour);
                                    }
                                }

                                if show_collision_areas {
                                    // TODO: Tidy up
                                    let game_object = object.clone().to_object();
                                    let poly = game_object.poly();

                                    for v in poly.verts.iter() {
                                        let rect = Rect::new(v.x, v.y, 10.0, 10.0)
                                            .move_position(game_position)
                                            .scale(scale);
                                        let model = Model::new(rect, None, 0.0, Flip::default());
                                        renderer.fill_rectangle(
                                            model,
                                            Colour::rgba(0.0, 1.0, 0.0, 0.5),
                                        );
                                    }

                                    let aabb = game_object.collision_aabb();
                                    let mut origin = game_object.origin();
                                    if let Some(area) = game_object.collision_area {
                                        origin =
                                            Vec2::new(origin.x - area.min.x, origin.y - area.min.y);
                                    }
                                    let rect =
                                        aabb.to_rect().move_position(game_position).scale(scale);
                                    let model = Model::new(
                                        rect,
                                        Some(origin * scale),
                                        object.angle,
                                        Flip::default(),
                                    );
                                    // TODO: model.move().scale()
                                    renderer
                                        .fill_rectangle(model, Colour::rgba(1.0, 0.0, 0.0, 0.5));
                                }
                            }
                        }
                    }
                }

                imgui_frame.render(&renderer.window);
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
