// TODO: Show origin debug option
// TODO: Enable selecting object again
// TODO: Exit fullscreen when in editor

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
    video::{GLContext, Window as SdlWindow},
    EventPump, VideoSubsystem,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashSet,
    error::Error,
    fs,
    path::Path,
    process, str,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

use sdlglue::{Model, Renderer, Texture};
use wee::{
    Action, Angle, AngleSetter, AnimationStatus, AnimationType, AssetFiles, Assets,
    BounceDirection, ButtonState, CollisionWith, CompassDirection, Completion, DrawIntroText,
    Effect, FontLoadInfo, GameData, GameSettings, ImageList, Images, Input, IntroFont,
    IntroFontConfig, IntroText, JumpLocation, LoadImages, LoadedGame, Motion, MouseInteraction,
    MouseOver, MovementDirection, MovementHandling, MovementType, PropertyCheck, PropertySetter,
    RelativeTo, RenderScene, SerialiseObject, SerialiseObjectList, Speed, SpeedCategory, Sprite,
    Switch, SwitchState, Target, TargetType, TextResize, Trigger, When, WinStatus,
    DEFAULT_GAME_SPEED,
};
use wee_common::{Colour, Flip, Rect, Vec2, WeeResult, AABB, PROJECTION_HEIGHT, PROJECTION_WIDTH};

const SMALL_BUTTON: [f32; 2] = [100.0, 50.0];
const NORMAL_BUTTON: [f32; 2] = [200.0, 50.0];

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

// TODO: to_string() and all that for Display is already implemented
trait ImguiDisplayTrigger {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images);
}

impl ImguiDisplayTrigger for Trigger {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images) {
        let image_tooltip = |image_name: &str| {
            if ui.is_item_hovered() {
                ui.tooltip(|| {
                    let texture_id = images[image_name].id;
                    imgui::Image::new(imgui::TextureId::from(texture_id as usize), [200.0, 200.0])
                        .build(ui);
                });
            }
        };

        let object_tooltip = |object_name: &str| {
            if ui.is_item_hovered() {
                ui.tooltip(|| match game.objects.get_obj(object_name) {
                    Ok(object) => match &object.sprite {
                        Sprite::Image { name: image_name } => {
                            if let Some(texture) = &images.get(image_name) {
                                let w = texture.width as f32;
                                let h = texture.height as f32;
                                let m = w.max(h);
                                let w = w / m * 200.0;
                                let h = h / m * 200.0;
                                imgui::Image::new(
                                    imgui::TextureId::from(texture.id as usize),
                                    [w, h],
                                )
                                .build(ui);
                            } else {
                                ui.text_colored(
                                    [1.0, 0.0, 0.0, 1.0],
                                    format!("Warning: Image `{}` not found", image_name),
                                );
                            }
                        }
                        Sprite::Colour(colour) => {
                            // TODO: Show colour
                        }
                    },
                    Err(_) => {
                        ui.text_colored(
                            [1.0, 0.0, 0.0, 1.0],
                            format!("Warning: Object `{}` not found", object_name),
                        );
                    }
                });
            }
        };

        let object_button_with_label = |label: &imgui::ImStr, object_name: &str| {
            if game.objects.iter().any(|o| o.name == object_name) {
                ui.text(label);
            } else {
                ui.text_colored([1.0, 0.0, 0.0, 1.0], label);
            }
            object_tooltip(object_name);
        };

        let object_button = |object_name: &str| {
            object_button_with_label(&imgui::ImString::from(object_name.to_string()), object_name);
        };

        let image_button_with_label = |label: &imgui::ImStr, image_name: &str| {
            ui.text(label);
            image_tooltip(image_name);
        };

        let image_button = |image_name: &str| {
            image_button_with_label(&imgui::ImString::from(image_name.to_string()), image_name);
        };

        let sprite_button = |sprite: &Sprite| {
            match sprite {
                Sprite::Image { name } => {
                    image_button(name);
                }
                Sprite::Colour(colour) => {
                    ui.text(format!(
                        "The colour {{ red: {}, green: {}, blue: {}, alpha: {} }}",
                        colour.r, colour.g, colour.b, colour.a
                    ));
                }
            };
        };

        let same_line = || ui.same_line_with_spacing(0.0, 3.0);

        match self {
            Trigger::Time(When::Start) => ui.text("When the game starts"),
            Trigger::Time(When::End) => ui.text("When the game ends"),
            Trigger::Time(When::Exact { time }) => {
                ui.text(format!("When the time is {:.2} seconds", time))
            }
            Trigger::Time(When::Random { start, end }) => ui.text(format!(
                "At a random time between {:.2} and {:.2} seconds",
                start, end,
            )),
            Trigger::Collision(CollisionWith::Object { name }) => {
                ui.text("While this object collides with");
                same_line();
                object_button(name);
            }
            Trigger::Collision(CollisionWith::Area(area)) => ui.text(format!(
                "While this object is inside {}, {} and {}, {}",
                area.min.x, area.min.y, area.max.x, area.max.y
            )),
            Trigger::WinStatus(status) => match status {
                WinStatus::Won => ui.text("While you have won the game"),
                WinStatus::Lost => ui.text("While you have lost the game"),
                WinStatus::NotYetWon => ui.text("While you haven't won"),
                WinStatus::NotYetLost => ui.text("While you haven't lost"),
                WinStatus::HasBeenWon => ui.text("When you win the game"),
                WinStatus::HasBeenLost => ui.text("When you lose the game"),
            },
            Trigger::Input(Input::Mouse { over, interaction }) => {
                if let MouseOver::Anywhere = over {
                    match interaction {
                        MouseInteraction::Button { state } => match state {
                            ButtonState::Press => ui.text("When the screen is clicked"),
                            ButtonState::Down => {
                                ui.text("While the mouse button is down");
                            }
                            ButtonState::Release => {
                                ui.text("When the mouse button is released");
                            }
                            ButtonState::Up => {
                                ui.text("While the mouse button isn't pressed");
                            }
                        },
                        MouseInteraction::Hover => {
                            ui.text("While the mouse hovers over the screen (always true)");
                        }
                    }
                } else {
                    let show_clicked_object = |clicked_object: &MouseOver| match clicked_object {
                        MouseOver::Object { name } => {
                            object_button(name);
                        }
                        MouseOver::Area(area) => {
                            ui.text(format!(
                                "the area between {}, {} and {}, {}",
                                area.min.x, area.min.y, area.max.x, area.max.y
                            ));
                        }
                        _ => {}
                    };
                    match interaction {
                        MouseInteraction::Button { state } => match state {
                            ButtonState::Press => {
                                ui.text("When");
                                same_line();
                                show_clicked_object(over);
                                same_line();
                                ui.text("is clicked")
                            }
                            ButtonState::Down => {
                                ui.text("While the mouse cursor is over");
                                same_line();
                                show_clicked_object(over);
                                same_line();
                                ui.text("and the mouse button is down");
                            }
                            ButtonState::Release => {
                                ui.text("When the mouse cursor is over");
                                same_line();
                                show_clicked_object(over);
                                same_line();
                                ui.text("and the mouse button is released");
                            }
                            ButtonState::Up => {
                                ui.text("While the mouse cursor is over");
                                same_line();
                                show_clicked_object(over);
                                same_line();
                                ui.text("and the mouse button is up");
                            }
                        },
                        MouseInteraction::Hover => {
                            ui.text("While the mouse is hovered over");
                            same_line();
                            show_clicked_object(over);
                        }
                    }
                }
            }
            Trigger::CheckProperty { name, check } => match check {
                PropertyCheck::Switch(switch) => match switch {
                    SwitchState::On => {
                        ui.text("While");
                        same_line();
                        object_button_with_label(&im_str!("{}'s", name), name);
                        same_line();
                        ui.text("switch is on");
                    }
                    SwitchState::Off => {
                        ui.text("While");
                        same_line();
                        object_button_with_label(&im_str!("{}'s", name), name);
                        same_line();
                        ui.text("switch is off");
                    }
                    SwitchState::SwitchedOn => {
                        ui.text("When");
                        same_line();
                        object_button(name);
                        same_line();
                        ui.text("is switched on")
                    }
                    SwitchState::SwitchedOff => {
                        ui.text("When");
                        same_line();
                        object_button(name);
                        same_line();
                        ui.text("is switched off")
                    }
                },
                PropertyCheck::Sprite(sprite) => {
                    ui.text("While");
                    same_line();
                    object_button_with_label(&im_str!("{}'s", name), name);
                    same_line();
                    ui.text("image is");
                    same_line();
                    sprite_button(sprite);
                }
                PropertyCheck::FinishedAnimation => {
                    ui.text("When");
                    same_line();
                    object_button_with_label(&im_str!("{}'s", name), name);
                    same_line();
                    ui.text("animation is finished");
                }
                PropertyCheck::Timer => ui.text("When this object's timer hits zero"),
            },
            Trigger::Random { chance } => ui.text(format!("With a {}% chance", chance * 100.0)),
        };
    }
}

trait ImguiDisplayAction {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images, indent: usize);
}

impl ImguiDisplayAction for Action {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images, indent: usize) {
        let image_tooltip = |image_name: &str| {
            if ui.is_item_hovered() {
                ui.tooltip(|| {
                    let texture_id = images[image_name].id;
                    imgui::Image::new(imgui::TextureId::from(texture_id as usize), [200.0, 200.0])
                        .build(ui);
                });
            }
        };

        let object_tooltip = |object_name: &str| {
            if ui.is_item_hovered() {
                ui.tooltip(|| match game.objects.get_obj(object_name) {
                    Ok(object) => match &object.sprite {
                        Sprite::Image { name: image_name } => {
                            if let Some(texture) = &images.get(image_name) {
                                let w = texture.width as f32;
                                let h = texture.height as f32;
                                let m = w.max(h);
                                let w = w / m * 200.0;
                                let h = h / m * 200.0;
                                imgui::Image::new(
                                    imgui::TextureId::from(texture.id as usize),
                                    [w, h],
                                )
                                .build(ui);
                            } else {
                                ui.text_colored(
                                    [1.0, 0.0, 0.0, 1.0],
                                    format!("Warning: Image `{}` not found", image_name),
                                );
                            }
                        }
                        Sprite::Colour(colour) => {
                            // TODO: Show colour
                        }
                    },
                    Err(_) => {
                        ui.text_colored(
                            [1.0, 0.0, 0.0, 1.0],
                            format!("Warning: Object `{}` not found", object_name),
                        );
                    }
                });
            }
        };

        let object_button_with_label = |label: &imgui::ImStr, object_name: &str| {
            if game.objects.iter().any(|o| o.name == object_name) {
                ui.text(label);
            } else {
                ui.text_colored([1.0, 0.0, 0.0, 1.0], label);
            }
            object_tooltip(object_name);
        };

        let object_button = |object_name: &str| {
            object_button_with_label(&imgui::ImString::from(object_name.to_string()), object_name);
        };

        let image_button_with_label = |label: &imgui::ImStr, image_name: &str| {
            ui.text(label);
            image_tooltip(image_name);
        };

        let image_button = |image_name: &str| {
            image_button_with_label(&imgui::ImString::from(image_name.to_string()), image_name);
        };

        let sprite_button = |sprite: &Sprite| {
            match sprite {
                Sprite::Image { name } => {
                    image_button(name);
                }
                Sprite::Colour(colour) => {
                    ui.text(format!(
                        "The colour {{ red: {}, green: {}, blue: {}, alpha: {} }}",
                        colour.r, colour.g, colour.b, colour.a
                    ));
                }
            };
        };

        let speed_to_string = |speed: &Speed| match speed {
            Speed::Category(SpeedCategory::VerySlow) => "very slow".to_string(),
            Speed::Category(SpeedCategory::Slow) => "slow".to_string(),
            Speed::Category(SpeedCategory::Normal) => "normal".to_string(),
            Speed::Category(SpeedCategory::Fast) => "fast".to_string(),
            Speed::Category(SpeedCategory::VeryFast) => "very fast".to_string(),
            Speed::Value(value) => format!("speed: {}", value),
        };

        let same_line = || ui.same_line_with_spacing(0.0, 3.0);
        let dir_to_str = |dir| match dir {
            CompassDirection::Up => "up",
            CompassDirection::UpRight => "up-right",
            CompassDirection::Right => "right",
            CompassDirection::DownRight => "down-right",
            CompassDirection::Down => "down",
            CompassDirection::DownLeft => "down-left",
            CompassDirection::Left => "left",
            CompassDirection::UpLeft => "up-left",
        };

        // TODO: Remove this
        use wee::*;

        ui.text("\t".repeat(indent));
        ui.same_line_with_spacing(0.0, 0.0);

        match self {
            Action::Motion(motion) => motion.display(ui, game, images),
            Action::Win => ui.text("Win the game"),
            Action::Lose => ui.text("Lose the game"),
            Action::Effect(effect) => match effect {
                Effect::Freeze => ui.text("Freeze the screen"),
                Effect::None => ui.text("Remove screen effects"),
            },
            Action::PlaySound { name } => ui.text(format!("Play the {} sound", name)),
            Action::StopMusic => ui.text("Stop the music"),
            Action::Animate {
                animation_type,
                sprites,
                speed,
            } => {
                let speed = speed_to_string(speed);
                if let AnimationType::Loop = animation_type {
                    ui.text(format!("Loops an animation {}", speed));
                } else {
                    ui.text(format!("Plays an animation once {}", speed));
                }
            }
            Action::DrawText {
                text,
                font,
                colour,
                resize,
            } => {
                let change_size = if let TextResize::MatchText = resize {
                    " changing this object's size to match the text size"
                } else {
                    ""
                };
                let colour = format!(
                    "red: {}, green: {}, blue: {}, alpha: {}",
                    colour.r, colour.g, colour.b, colour.a
                );
                ui.text(format!(
                    "Draws `{}` using the {} font with colour ({}) {}",
                    text, font, colour, change_size
                ));
            }
            Action::SetProperty(PropertySetter::Angle(angle_setter)) => {
                match angle_setter {
                    AngleSetter::Value(value) => {
                        ui.text(format!("Set this object's angle to {}", value))
                    }
                    AngleSetter::Increase(value) => {
                        ui.text(format!("Increase this object's angle by {}", value))
                    }
                    AngleSetter::Decrease(value) => {
                        ui.text(format!("Decrease this object's angle by {}", value))
                    }
                    AngleSetter::Match { name } => {
                        ui.text(format!("Have the same angle as {}", name))
                    }
                    AngleSetter::Clamp { min, max } => ui.text(format!(
                        "Clamp this object's angle to between {} and {} degrees",
                        min, max
                    )),
                    AngleSetter::RotateToMouse => ui.text("Rotate towards the mouse"),
                };
            }
            Action::SetProperty(PropertySetter::Sprite(sprite)) => {
                ui.text(format!("Set this object's sprite to {:?}", sprite))
            }
            Action::SetProperty(PropertySetter::Size(size_setter)) => {
                match size_setter {
                    SizeSetter::Value(size) => ui.text(format!(
                        "Set this object's width to {} and its height to {}",
                        size.width, size.height
                    )),
                    SizeSetter::Grow(SizeDifference::Value(size)) => ui.text(format!(
                        "Grow this object's width by {}px and its height by {}px",
                        size.width, size.height
                    )),
                    SizeSetter::Shrink(SizeDifference::Value(size)) => ui.text(format!(
                        "Shrink this object's width by {}px and its height by {}px",
                        size.width, size.height
                    )),
                    SizeSetter::Grow(SizeDifference::Percent(percent)) => ui.text(format!(
                        "Grow this object's width by {}% and its height by {}%",
                        percent.width * 100.0,
                        percent.height * 100.0
                    )),
                    SizeSetter::Shrink(SizeDifference::Percent(percent)) => ui.text(format!(
                        "Shrink this object's width by {}% and its height by {}%",
                        percent.width * 100.0,
                        percent.height * 100.0
                    )),
                    SizeSetter::Clamp { min, max } => ui.text(format!(
                        "Clamp this object's size between {}x{} pixels and {}x{} pixels",
                        min.width, min.height, max.width, max.height
                    )),
                };
            }
            Action::SetProperty(PropertySetter::Switch(switch)) => ui.text(format!(
                "Set the switch {}",
                if let Switch::On = switch { "on" } else { "off" }
            )),
            Action::SetProperty(PropertySetter::Timer { time }) => {
                ui.text(format!("Set the timer to {:.2} seconds ", time))
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::Flip)) => {
                ui.text("Flip this object horizontally")
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::Flip)) => {
                ui.text("Flip this object vertically")
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::SetFlip(flipped))) => {
                ui.text(format!("Set this object's horizontal flip to {}", flipped))
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::SetFlip(flipped))) => {
                ui.text(format!("Set this object's vertical flip to {}", flipped))
            }
            Action::SetProperty(PropertySetter::Layer(layer_setter)) => {
                match layer_setter {
                    LayerSetter::Value(value) => {
                        ui.text(format!("Set this object's layer to {}", value))
                    }
                    LayerSetter::Increase => ui.text("Increase this object's layer by 1"),
                    LayerSetter::Decrease => ui.text("Decrease this object's layer by 1"),
                };
            }
            Action::Random { random_actions } => {
                ui.text("Chooses a random action");
                for action in random_actions {
                    action.display(ui, game, images, indent + 1);
                }
            }
        };
    }
}

trait ImguiDisplayMotion {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images);
}

impl ImguiDisplayMotion for Motion {
    fn display(&self, ui: &imgui::Ui, game: &GameData, images: &Images) {
        let speed_to_string = |speed: &Speed| match speed {
            Speed::Category(SpeedCategory::VerySlow) => "very slow".to_string(),
            Speed::Category(SpeedCategory::Slow) => "slow".to_string(),
            Speed::Category(SpeedCategory::Normal) => "normal".to_string(),
            Speed::Category(SpeedCategory::Fast) => "fast".to_string(),
            Speed::Category(SpeedCategory::VeryFast) => "very fast".to_string(),
            Speed::Value(value) => format!("speed: {}", value),
        };

        let same_line = || ui.same_line_with_spacing(0.0, 3.0);
        let dir_to_str = |dir| match dir {
            CompassDirection::Up => "up",
            CompassDirection::UpRight => "up-right",
            CompassDirection::Right => "right",
            CompassDirection::DownRight => "down-right",
            CompassDirection::Down => "down",
            CompassDirection::DownLeft => "down-left",
            CompassDirection::Left => "left",
            CompassDirection::UpLeft => "up-left",
        };

        let angle_to_string = |angle: Angle| match angle {
            Angle::Current => "in the current angle".to_string(),
            Angle::Degrees(angle) => format!("at a {} degree angle", angle),
            Angle::Random { min, max } => format!("in a random angle between {} and {}", min, max),
        };

        // TODO: Remove this
        use wee::*;

        match self {
            Motion::Stop => ui.text("Stop this object"),
            Motion::GoStraight { direction, speed } => {
                let speed = speed_to_string(speed);
                match direction {
                    MovementDirection::Direction {
                        possible_directions: dirs,
                    } => {
                        if dirs.is_empty() {
                            ui.text(format!("Go {} in a random direction", speed));
                        } else if dirs.len() == 1 {
                            ui.text(format!(
                                "Go {} {}",
                                dir_to_str(*dirs.iter().next().unwrap()),
                                speed
                            ));
                        } else {
                            let dirs: Vec<&str> = dirs.iter().map(|dir| dir_to_str(*dir)).collect();
                            let dirs = dirs.join(", ");
                            ui.text(format!("Go {} in a direction chosen from {}", speed, dirs));
                        }
                    }
                    MovementDirection::Angle(angle) => match angle {
                        Angle::Current => {
                            ui.text(format!("Go {} in this object's current angle", speed))
                        }

                        Angle::Degrees(angle) => {
                            ui.text(format!("Go {} at a {} angle", speed, angle))
                        }
                        Angle::Random { min, max } => ui.text(format!(
                            "Go {} at a random angle between {} and {}",
                            speed, min, max
                        )),
                    },
                }
            }
            Motion::JumpTo(jump_location) => match jump_location {
                JumpLocation::Point(point) => {
                    ui.text(format!("Jump to {}, {}", point.x, point.y));
                }
                JumpLocation::Relative { to, distance } => match to {
                    RelativeTo::CurrentPosition => {
                        ui.text(format!(
                            "Jump {}, {} relative to this object's position",
                            distance.x, distance.y
                        ));
                    }
                    RelativeTo::CurrentAngle => {
                        ui.text(format!(
                            "Jump {}, {} relative to this object's angle",
                            distance.x, distance.y
                        ));
                    }
                },
                JumpLocation::Area(area) => {
                    ui.text(format!(
                        "Jump to a random location between {}, {} and {}, {}",
                        area.min.x, area.min.y, area.max.x, area.max.y
                    ));
                }
                JumpLocation::ClampPosition { area } => {
                    ui.text(format!(
                        "Clamp this object's position within {}, {} and {}, {}",
                        area.min.x, area.min.y, area.max.x, area.max.y
                    ));
                }
                JumpLocation::Object { name } => {
                    ui.text(format!("Jump to {}'s position", name));
                }
                JumpLocation::Mouse => {
                    ui.text("Jump to the mouse");
                }
            },
            Motion::Roam {
                movement_type,
                area,
                speed,
            } => {
                let speed = speed_to_string(speed);
                match movement_type {
                    MovementType::Wiggle => ui.text(format!(
                        "Wiggle {} between {}, {} and {}, {}",
                        speed, area.min.x, area.min.y, area.max.x, area.max.y,
                    )),
                    MovementType::Insect => ui.text(format!(
                        "Move like an insect {} between {}, {} and {}, {}",
                        speed, area.min.x, area.min.y, area.max.x, area.max.y,
                    )),
                    MovementType::Reflect {
                        movement_handling, ..
                    } => {
                        let movement_handling = match movement_handling {
                            MovementHandling::Anywhere => "",
                            MovementHandling::TryNotToOverlap => {
                                " trying not to overlap with over objects"
                            }
                        };
                        ui.text(format!(
                            "Reflect {} between {}, {} and {}, {}{}",
                            speed,
                            area.min.x,
                            area.min.y,
                            area.max.x,
                            area.max.y,
                            movement_handling,
                        ));
                    }
                    MovementType::Bounce { .. } => ui.text(format!(
                        "Move like an insect {} between {}, {} and {}, {}",
                        speed, area.min.x, area.min.y, area.max.x, area.max.y,
                    )),
                };
            }
            Motion::Swap { name } => ui.text(format!("Swap position with {}", name)),
            Motion::Target {
                target,
                target_type,
                offset,
                speed,
            } => {
                let name = match target {
                    Target::Object { name } => name,
                    Target::Mouse => "Mouse",
                };
                let offset = if *offset == Vec2::zero() {
                    "".to_string()
                } else {
                    format!(" with an offset of {}, {}", offset.x, offset.y)
                };
                match target_type {
                    TargetType::Follow => {
                        ui.text(format!(
                            "Follow {} {}{}",
                            name,
                            speed_to_string(speed),
                            offset
                        ));
                    }
                    TargetType::StopWhenReached => {
                        ui.text(format!(
                            "Target {} {}{}",
                            name,
                            speed_to_string(speed),
                            offset
                        ));
                    }
                }
            }
            Motion::Accelerate { direction, speed } => {
                let speed = speed_to_string(speed);
                match direction {
                    MovementDirection::Direction {
                        possible_directions: dirs,
                    } => {
                        if dirs.is_empty() {
                            ui.text(format!("Accelerate {} in a random direction", speed));
                        } else if dirs.len() == 1 {
                            ui.text(format!(
                                "Accelerate {} {}",
                                dir_to_str(*dirs.iter().next().unwrap()),
                                speed
                            ));
                        } else {
                            let dirs: Vec<&str> = dirs.iter().map(|dir| dir_to_str(*dir)).collect();
                            let dirs = dirs.join(", ");
                            ui.text(format!(
                                "Accelerate {} in a direction chosen from {}",
                                speed, dirs
                            ));
                        }
                    }
                    MovementDirection::Angle(angle) => match angle {
                        Angle::Current => ui.text(format!(
                            "Accelerate {} in this object's current angle",
                            speed
                        )),

                        Angle::Degrees(angle) => {
                            ui.text(format!("Accelerate {} at a {} angle", speed, angle))
                        }
                        Angle::Random { min, max } => ui.text(format!(
                            "Accelerate {} at a random angle between {} and {}",
                            speed, min, max
                        )),
                    },
                }
            }
        };
    }
}

fn right_window(
    ui: &imgui::Ui,
    game: &mut GameData,
    images: &mut Images,
    filename: &Option<String>,
    selected_index: &Option<usize>,
    instruction_mode: &mut InstructionMode,
    instruction_index: &mut usize,
    instruction_focus: &mut InstructionFocus,
) {
    imgui::ChildWindow::new(im_str!("Right"))
        .size([0.0, 0.0])
        .scroll_bar(false)
        .build(ui, || {
            fn tab_bar<F: FnOnce()>(label: &imgui::ImStr, f: F) {
                unsafe {
                    if imgui::sys::igBeginTabBar(label.as_ptr(), 0) {
                        f();
                        imgui::sys::igEndTabItem();
                    }
                }
            }

            fn tab_item<F: FnOnce()>(label: &imgui::ImStr, f: F) {
                unsafe {
                    if imgui::sys::igBeginTabItem(
                        label.as_ptr(),
                        std::ptr::null_mut(),
                        0 as ::std::os::raw::c_int,
                    ) {
                        f();
                        imgui::sys::igEndTabItem()
                    }
                }
            }
            if let Some(index) = selected_index {
                tab_bar(im_str!("Tab Bar"), || {
                    tab_item(im_str!("Properties"), || {
                        properties_window(ui, game, *index, images, filename);
                    });
                    tab_item(im_str!("Instructions"), || {
                        instruction_window(
                            ui,
                            game,
                            *index,
                            images,
                            filename,
                            instruction_mode,
                            instruction_index,
                            instruction_focus,
                        );
                    });
                });
            }
        });
}

enum InstructionMode {
    View,
    Edit,
    AddTrigger,
    AddAction,
    EditTrigger,
    EditAction,
}

#[derive(Debug, PartialEq)]
enum InstructionFocus {
    Trigger { index: usize },
    Action { index: usize },
    None,
}

fn instruction_window(
    ui: &imgui::Ui,
    game: &mut GameData,
    index: usize,
    images: &mut Images,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
    instruction_index: &mut usize,
    focus: &mut InstructionFocus,
) {
    match instruction_mode {
        InstructionMode::View => {
            imgui::ChildWindow::new(im_str!("Test"))
                .size([0.0, -ui.frame_height_with_spacing()])
                //.border(true)
                //.horizontal_scrollbar(true)
                .horizontal_scrollbar(true)
                .build(&ui, || {
                    for (i, instruction) in
                        game.objects[index].instructions.clone().iter().enumerate()
                    {
                        ui.tree_node(&im_str!("Instruction {}", i + 1))
                            .flags(imgui::ImGuiTreeNodeFlags::SpanAvailWidth)
                            .bullet(true)
                            .leaf(true)
                            .selected(*instruction_index == i)
                            .build(|| {
                                if ui.is_item_clicked(imgui::MouseButton::Left) {
                                    *instruction_index = i;
                                }
                                if instruction.triggers.is_empty() {
                                    ui.text("\tEvery frame")
                                } else {
                                    for trigger in instruction.triggers.iter() {
                                        trigger.display(ui, game, images);
                                    }
                                }

                                ui.text("\t\tThen:");

                                if instruction.actions.is_empty() {
                                    ui.text("\tDo nothing")
                                } else {
                                    for action in instruction.actions.iter() {
                                        action.display(ui, game, images, 0);
                                    }
                                }
                            });
                        ui.separator();
                    }
                });
            if ui.small_button(im_str!("Up")) && *instruction_index > 0 {
                game.objects[index]
                    .instructions
                    .swap(*instruction_index, *instruction_index - 1);
                *instruction_index -= 1;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Down"))
                && *instruction_index + 1 < game.objects[index].instructions.len()
            {
                game.objects[index]
                    .instructions
                    .swap(*instruction_index, *instruction_index + 1);
                *instruction_index += 1;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Add")) {
                game.objects[index].instructions.push(wee::Instruction {
                    triggers: Vec::new(),
                    actions: Vec::new(),
                });
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Edit")) {
                *instruction_mode = InstructionMode::Edit;
                /*self.mode = InstructionEditorMode::EditInstruction {
                    focus: InstructionFocus::Trigger,
                    selected_index: 0,
                    selected_sub_index: None,
                };*/
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Delete")) && !game.objects[index].instructions.is_empty() {
                game.objects[index].instructions.remove(*instruction_index);
                if *instruction_index > 0 {
                    *instruction_index -= 1;
                }
            }
        }
        InstructionMode::Edit => {
            ui.text("Triggers:");
            let instruction = &mut game.objects[index].instructions[*instruction_index];
            for (i, trigger) in instruction.triggers.iter().enumerate() {
                let selected = InstructionFocus::Trigger { index: i } == *focus;
                if imgui::Selectable::new(&ImString::new(trigger.to_string()))
                    .selected(selected)
                    .build(&ui)
                {
                    *focus = InstructionFocus::Trigger { index: i };
                }
            }
            if ui.button(im_str!("Add Trigger"), SMALL_BUTTON) {
                *instruction_mode = InstructionMode::AddTrigger;
            }
            ui.text("Actions:");
            for (i, action) in instruction.actions.iter().enumerate() {
                let selected = InstructionFocus::Action { index: i } == *focus;
                if imgui::Selectable::new(&ImString::new(action.to_string()))
                    .selected(selected)
                    .build(&ui)
                {
                    *focus = InstructionFocus::Action { index: i };
                }
                match action {
                    Action::Random { random_actions } => {
                        for action in random_actions.iter() {
                            let selected = InstructionFocus::Action { index: i } == *focus;
                            if imgui::Selectable::new(&ImString::new(format!("\t{}", action)))
                                .selected(selected)
                                .build(&ui)
                            {
                                *focus = InstructionFocus::Action { index: i };
                            }
                        }
                    }
                    _ => {}
                }
            }
            if ui.button(im_str!("Add Action"), SMALL_BUTTON) {
                *instruction_mode = InstructionMode::AddAction;
            }
            if ui.small_button(im_str!("Back")) {
                *instruction_mode = InstructionMode::View;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Up")) {
                match focus {
                    InstructionFocus::Trigger {
                        index: selected_index,
                    } => {
                        if *selected_index > 0 {
                            instruction
                                .triggers
                                .swap(*selected_index, *selected_index - 1);
                            *selected_index -= 1;
                        }
                    }
                    InstructionFocus::Action {
                        index: selected_index,
                    } => {
                        if *selected_index > 0 {
                            instruction
                                .actions
                                .swap(*selected_index, *selected_index - 1);
                            *selected_index -= 1;
                        }
                    }
                    InstructionFocus::None => {}
                }
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Down")) {
                match focus {
                    InstructionFocus::Trigger {
                        index: selected_index,
                    } => {
                        if *selected_index + 1 < instruction.triggers.len() {
                            instruction
                                .triggers
                                .swap(*selected_index, *selected_index + 1);
                            *selected_index += 1;
                        }
                    }
                    InstructionFocus::Action {
                        index: selected_index,
                    } => {
                        if *selected_index + 1 < instruction.actions.len() {
                            instruction
                                .actions
                                .swap(*selected_index, *selected_index + 1);
                            *selected_index += 1;
                        }
                    }
                    InstructionFocus::None => {}
                }
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Edit")) {
                match focus {
                    InstructionFocus::Trigger { .. } => {
                        *instruction_mode = InstructionMode::EditTrigger;
                    }
                    InstructionFocus::Action { .. } => {
                        *instruction_mode = InstructionMode::EditAction;
                    }
                    InstructionFocus::None => {}
                }
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Delete")) {
                match focus {
                    InstructionFocus::Trigger {
                        index: selected_index,
                    } => {
                        if !instruction.triggers.is_empty() {
                            instruction.triggers.remove(*selected_index);
                            if *selected_index > 0 {
                                *selected_index -= 1;
                            }
                        }
                    }
                    InstructionFocus::Action {
                        index: selected_index,
                    } => {
                        if !instruction.actions.is_empty() {
                            instruction.actions.remove(*selected_index);
                            if *selected_index > 0 {
                                *selected_index -= 1;
                            }
                        }
                    }
                    InstructionFocus::None => {}
                }
            }
        }
        InstructionMode::AddTrigger => {
            let instruction = &mut game.objects[index].instructions[*instruction_index];
            instruction.triggers.push(Trigger::Time(When::Start));
            *focus = InstructionFocus::Trigger {
                index: instruction.triggers.len() - 1,
            };
            *instruction_mode = InstructionMode::EditTrigger;
        }
        InstructionMode::AddAction => {
            let instruction = &mut game.objects[index].instructions[*instruction_index];
            instruction.actions.push(Action::Win);
            *focus = InstructionFocus::Action {
                index: instruction.actions.len() - 1,
            };
            *instruction_mode = InstructionMode::EditAction;
        }
        InstructionMode::EditTrigger => {
            let object_names: Vec<String> = game.objects.iter().map(|o| o.name.clone()).collect();
            let trigger_index = match focus {
                InstructionFocus::Trigger { index } => *index,
                _ => unreachable!(),
            };
            let trigger =
                &mut game.objects[index].instructions[*instruction_index].triggers[trigger_index];
            let mut current_trigger_position = match trigger {
                Trigger::Time(_) => 0,
                Trigger::Collision(_) => 1,
                Trigger::Input(_) => 2,
                Trigger::WinStatus(_) => 3,
                Trigger::Random { .. } => 4,
                Trigger::CheckProperty {
                    check: PropertyCheck::Switch(_),
                    ..
                } => 5,
                Trigger::CheckProperty {
                    check: PropertyCheck::Sprite(_),
                    ..
                } => 6,
                Trigger::CheckProperty {
                    check: PropertyCheck::FinishedAnimation,
                    ..
                } => 7,
                Trigger::CheckProperty {
                    check: PropertyCheck::Timer,
                    ..
                } => 8,
            };
            let trigger_names = [
                im_str!("Time"),
                im_str!("Collision"),
                im_str!("Mouse"),
                im_str!("Win Status"),
                im_str!("Random Chance"),
                im_str!("Check Switch"),
                im_str!("Check Sprite"),
                im_str!("Finished Animation"),
                im_str!("Timer"),
            ];
            if imgui::ComboBox::new(im_str!("Trigger")).build_simple_string(
                &ui,
                &mut current_trigger_position,
                &trigger_names,
            ) {
                *trigger = match current_trigger_position {
                    0 => Trigger::Time(When::Start),
                    1 => {
                        Trigger::Collision(CollisionWith::Area(AABB::new(0.0, 0.0, 1600.0, 900.0)))
                    }
                    2 => Trigger::Input(Input::Mouse {
                        over: MouseOver::Anywhere,
                        interaction: MouseInteraction::Hover,
                    }),
                    3 => Trigger::WinStatus(WinStatus::Won),
                    4 => Trigger::Random { chance: 0.5 },
                    5 => Trigger::CheckProperty {
                        name: object_names[0].clone(),
                        check: PropertyCheck::Switch(SwitchState::On),
                    },
                    6 => Trigger::CheckProperty {
                        name: object_names[0].clone(),
                        check: PropertyCheck::Sprite(Sprite::Colour(Colour::black())),
                    },
                    7 => Trigger::CheckProperty {
                        name: object_names[0].clone(),
                        check: PropertyCheck::FinishedAnimation,
                    },
                    8 => Trigger::CheckProperty {
                        name: object_names[0].clone(),
                        check: PropertyCheck::Timer,
                    },
                    _ => unreachable!(),
                }
            }
            match trigger {
                Trigger::Time(when) => {
                    let mut current_when_position = match when {
                        When::Start => 0,
                        When::End => 1,
                        When::Exact { .. } => 2,
                        When::Random { .. } => 3,
                    };
                    let when_names = [
                        im_str!("Start"),
                        im_str!("End"),
                        im_str!("Exact Time"),
                        im_str!("Random Time"),
                    ];
                    if imgui::ComboBox::new(im_str!("When")).build_simple_string(
                        &ui,
                        &mut current_when_position,
                        &when_names,
                    ) {
                        *when = match current_when_position {
                            0 => When::Start,
                            1 => When::End,
                            2 => When::Exact { time: 0 },
                            3 => When::Random { start: 0, end: 60 },
                            _ => unreachable!(),
                        };
                    }

                    match when {
                        When::Exact { time } => {
                            let mut changed_time = *time as i32;
                            ui.input_int(im_str!("Time"), &mut changed_time).build();
                            *time = changed_time as u32;
                        }
                        When::Random { start, end } => {
                            let mut frames = [*start as i32, *end as i32];
                            ui.drag_int2(im_str!("Time"), &mut frames)
                                .min(0)
                                .max((game.length * 60.0) as i32)
                                .build();
                            *start = frames[0] as u32;
                            *end = frames[1] as u32;
                        }
                        _ => {}
                    }
                }
                Trigger::Collision(with) => {
                    let mut collision_type = if let CollisionWith::Object { .. } = with {
                        0
                    } else {
                        1
                    };
                    let collision_typename = if collision_type == 0 {
                        "Object".to_string()
                    } else {
                        "Area".to_string()
                    };
                    if imgui::Slider::new(
                        im_str!("Collision With"),
                        std::ops::RangeInclusive::new(0, 1),
                    )
                    .display_format(&ImString::from(collision_typename))
                    .build(&ui, &mut collision_type)
                    {
                        *with = if collision_type == 0 {
                            CollisionWith::Object {
                                name: object_names[0].clone(),
                            }
                        } else {
                            CollisionWith::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))
                        };
                    }

                    match with {
                        CollisionWith::Object { name } => {
                            let mut current_object = object_names
                                .iter()
                                .position(|obj_name| obj_name == name)
                                .unwrap();
                            let keys: Vec<ImString> = object_names
                                .iter()
                                .map(|name| ImString::from(name.clone()))
                                .collect();
                            let combo_keys: Vec<_> = keys.iter().collect();
                            if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                                &ui,
                                &mut current_object,
                                &combo_keys,
                            ) {
                                *name = object_names[current_object].clone();
                            }
                        }
                        CollisionWith::Area(area) => {
                            ui.input_float(im_str!("Collision Min X"), &mut area.min.x)
                                .build();
                            ui.input_float(im_str!("Collision Min Y"), &mut area.min.y)
                                .build();
                            ui.input_float(im_str!("Collision Max X"), &mut area.max.x)
                                .build();
                            ui.input_float(im_str!("Collision Max Y"), &mut area.max.y)
                                .build();
                        }
                    }
                }
                Trigger::Input(Input::Mouse { over, interaction }) => {
                    let mut input_type = match over {
                        MouseOver::Object { .. } => 0,
                        MouseOver::Area(_) => 1,
                        MouseOver::Anywhere => 2,
                    };
                    let input_typename = if input_type == 0 {
                        "Object".to_string()
                    } else {
                        "Area".to_string()
                    };
                    if imgui::Slider::new(
                        im_str!("Mouse Over"),
                        std::ops::RangeInclusive::new(0, 1),
                    )
                    .display_format(&ImString::from(input_typename))
                    .build(&ui, &mut input_type)
                    {
                        *over = if input_type == 0 {
                            MouseOver::Object {
                                name: object_names[0].clone(),
                            }
                        } else if input_type == 1 {
                            MouseOver::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))
                        } else {
                            MouseOver::Anywhere
                        };
                    }
                    match over {
                        MouseOver::Object { name } => {
                            let mut current_object = object_names
                                .iter()
                                .position(|obj_name| obj_name == name)
                                .unwrap();
                            let keys: Vec<ImString> = object_names
                                .iter()
                                .map(|name| ImString::from(name.clone()))
                                .collect();
                            let combo_keys: Vec<_> = keys.iter().collect();
                            if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                                &ui,
                                &mut current_object,
                                &combo_keys,
                            ) {
                                *name = object_names[current_object].clone();
                            }
                        }
                        MouseOver::Area(area) => {
                            ui.input_float(im_str!("Area Min X"), &mut area.min.x)
                                .build();
                            ui.input_float(im_str!("Area Min Y"), &mut area.min.y)
                                .build();
                            ui.input_float(im_str!("Area Max X"), &mut area.max.x)
                                .build();
                            ui.input_float(im_str!("Area Max Y"), &mut area.max.y)
                                .build();
                        }
                        _ => {}
                    }
                    let button_states = [
                        im_str!("Button Up"),
                        im_str!("Button Down"),
                        im_str!("Button Press"),
                        im_str!("Button Release"),
                        im_str!("Hover"),
                    ];
                    let mut current_button_state = match interaction {
                        MouseInteraction::Button { state } => *state as usize,
                        MouseInteraction::Hover => 4,
                    };
                    if imgui::ComboBox::new(im_str!("Button State")).build_simple_string(
                        &ui,
                        &mut current_button_state,
                        &button_states,
                    ) {
                        *interaction = match current_button_state {
                            0 => MouseInteraction::Button {
                                state: ButtonState::Up,
                            },
                            1 => MouseInteraction::Button {
                                state: ButtonState::Down,
                            },
                            2 => MouseInteraction::Button {
                                state: ButtonState::Press,
                            },
                            3 => MouseInteraction::Button {
                                state: ButtonState::Release,
                            },
                            4 => MouseInteraction::Hover,
                            _ => unreachable!(),
                        };
                    }
                }
                Trigger::WinStatus(status) => {
                    let win_states = [
                        im_str!("Won"),
                        im_str!("Lost"),
                        im_str!("Has Been Won"),
                        im_str!("Has Been Lost"),
                        im_str!("Not Yet Won"),
                        im_str!("Not Yet Lost"),
                    ];
                    let mut current_status = *status as usize;

                    if imgui::ComboBox::new(im_str!("Win State")).build_simple_string(
                        &ui,
                        &mut current_status,
                        &win_states,
                    ) {
                        *status = match current_status {
                            0 => WinStatus::Won,
                            1 => WinStatus::Lost,
                            2 => WinStatus::HasBeenWon,
                            3 => WinStatus::HasBeenLost,
                            4 => WinStatus::NotYetWon,
                            5 => WinStatus::NotYetLost,
                            _ => WinStatus::Won,
                        };
                    }
                }
                Trigger::Random { chance } => {
                    let mut chance_percent = *chance * 100.0;
                    ui.drag_float(im_str!("Chance"), &mut chance_percent)
                        .min(0.0)
                        .max(100.0)
                        .speed(1.0)
                        .display_format(im_str!("%.01f%%"))
                        .build();
                    *chance = chance_percent / 100.0;
                }
                Trigger::CheckProperty { name, check } => {
                    let mut current_object = object_names
                        .iter()
                        .position(|obj_name| obj_name == name)
                        .unwrap();
                    let keys: Vec<ImString> = object_names
                        .iter()
                        .map(|name| ImString::from(name.clone()))
                        .collect();
                    let combo_keys: Vec<_> = keys.iter().collect();
                    if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                        &ui,
                        &mut current_object,
                        &combo_keys,
                    ) {
                        *name = object_names[current_object].clone();
                    }
                    match check {
                        PropertyCheck::Switch(switch_state) => {
                            let switch_states = [
                                im_str!("On"),
                                im_str!("Off"),
                                im_str!("Switched On"),
                                im_str!("Switched Off"),
                            ];

                            let mut current_switch_state = *switch_state as usize;

                            if imgui::ComboBox::new(im_str!("Switch State")).build_simple_string(
                                &ui,
                                &mut current_switch_state,
                                &switch_states,
                            ) {
                                *switch_state = match current_switch_state {
                                    0 => SwitchState::On,
                                    1 => SwitchState::Off,
                                    2 => SwitchState::SwitchedOn,
                                    3 => SwitchState::SwitchedOff,
                                    _ => SwitchState::On,
                                };
                            }
                        }
                        PropertyCheck::Sprite(sprite) => {
                            let mut sprite_type = if let Sprite::Image { .. } = sprite {
                                0
                            } else {
                                1
                            };
                            let sprite_typename = if sprite_type == 0 {
                                "Image".to_string()
                            } else {
                                "Colour".to_string()
                            };
                            if imgui::Slider::new(
                                im_str!("Sprite"),
                                std::ops::RangeInclusive::new(0, 1),
                            )
                            .display_format(&ImString::from(sprite_typename))
                            .build(&ui, &mut sprite_type)
                            {
                                *sprite = if sprite_type == 0 {
                                    Sprite::Image {
                                        name: images.keys().next().unwrap().clone(),
                                    }
                                } else {
                                    Sprite::Colour(Colour::black())
                                };
                            }

                            match sprite {
                                Sprite::Image { name: image_name } => {
                                    // TODO: Tidy up
                                    let mut current_image =
                                        images.keys().position(|k| k == image_name).unwrap_or(0);
                                    let path = match filename {
                                        Some(filename) => {
                                            Path::new(filename).parent().unwrap().join("images")
                                        }
                                        None => Path::new("games").to_owned(),
                                    };

                                    if images.is_empty() {
                                        if ui.button(im_str!("Add a New Image"), NORMAL_BUTTON) {
                                            let first_key = choose_image_from_files(
                                                &mut game.asset_files,
                                                images,
                                                path,
                                            );
                                            match first_key {
                                                Some(key) => {
                                                    *sprite = Sprite::Image { name: key.clone() };
                                                }
                                                None => {
                                                    log::error!(
                                                        "None of the new images loaded correctly"
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        let mut keys: Vec<ImString> = images
                                            .keys()
                                            .map(|k| ImString::from(k.clone()))
                                            .collect();

                                        keys.push(ImString::new("Add a New Image"));

                                        let image_names: Vec<&ImString> = keys.iter().collect();

                                        if imgui::ComboBox::new(im_str!("Image"))
                                            .build_simple_string(
                                                &ui,
                                                &mut current_image,
                                                &image_names,
                                            )
                                        {
                                            if current_image == image_names.len() - 1 {
                                                let first_key = choose_image_from_files(
                                                    &mut game.asset_files,
                                                    images,
                                                    path,
                                                );
                                                match first_key {
                                                    Some(key) => {
                                                        *sprite =
                                                            Sprite::Image { name: key.clone() };
                                                    }
                                                    None => {
                                                        log::error!("None of the new images loaded correctly");
                                                    }
                                                }
                                            } else {
                                                match images.keys().nth(current_image) {
                                                    Some(image) => {
                                                        *sprite = Sprite::Image {
                                                            name: image.clone(),
                                                        };
                                                    }
                                                    None => {
                                                        log::error!(
                                                            "Could not set image to index {}",
                                                            current_image
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    };
                                }
                                Sprite::Colour(colour) => {
                                    let mut colour_array = [colour.r, colour.g, colour.b, colour.a];
                                    imgui::ColorEdit::new(im_str!("Colour"), &mut colour_array)
                                        .alpha(true)
                                        .build(ui);
                                    *colour = Colour::rgba(
                                        colour_array[0],
                                        colour_array[1],
                                        colour_array[2],
                                        colour_array[3],
                                    );
                                }
                            };
                        }
                        _ => {}
                    }
                }
            }
            if ui.small_button(im_str!("Back")) {
                *instruction_mode = InstructionMode::Edit;
            }
        }
        InstructionMode::EditAction => {
            let object_names: Vec<String> = game.objects.iter().map(|o| o.name.clone()).collect();
            let action_index = match focus {
                InstructionFocus::Action { index } => *index,
                _ => unreachable!(),
            };
            let action =
                &mut game.objects[index].instructions[*instruction_index].actions[action_index];
            let mut current_action_position = match action {
                Action::Win => 0,
                Action::Lose => 1,
                Action::Effect(_) => 2,
                Action::Motion(_) => 3,
                Action::PlaySound { .. } => 4,
                Action::StopMusic => 5,
                Action::SetProperty(_) => 6,
                Action::Animate { .. } => 7,
                Action::DrawText { .. } => 8,
                Action::Random { .. } => 9,
            };
            let action_names = [
                im_str!("Win"),
                im_str!("Lose"),
                im_str!("Effect"),
                im_str!("Motion"),
                im_str!("Play Sound"),
                im_str!("Stop Music"),
                im_str!("Set Property"),
                im_str!("Animate"),
                im_str!("Draw Text"),
                im_str!("Random Action"),
            ];
            if imgui::ComboBox::new(im_str!("Action")).build_simple_string(
                &ui,
                &mut current_action_position,
                &action_names,
            ) {
                *action = match current_action_position {
                    0 => Action::Win,
                    1 => Action::Lose,
                    2 => Action::Effect(Effect::None),
                    3 => Action::Motion(Motion::Stop),
                    4 => Action::PlaySound {
                        name: "".to_string(),
                    }, // TODO: assets.sounds.first
                    5 => Action::StopMusic,
                    6 => Action::SetProperty(PropertySetter::Angle(AngleSetter::Value(0.0))),
                    7 => Action::Animate {
                        animation_type: AnimationType::Loop,
                        sprites: Vec::new(),
                        speed: Speed::Category(SpeedCategory::Normal),
                    },
                    8 => Action::DrawText {
                        text: "".to_string(),
                        font: "".to_string(), // TODO: assets.fonts.first
                        colour: Colour::black(),
                        resize: TextResize::MatchObject,
                    },
                    9 => Action::Random {
                        random_actions: Vec::new(),
                    },
                    _ => unreachable!(),
                }
            }

            match action {
                Action::Effect(effect) => {
                    let mut effect_type = if *effect == Effect::None { 0 } else { 1 };
                    let effect_typename = if effect_type == 0 {
                        "None".to_string()
                    } else {
                        "Freeze".to_string()
                    };
                    if imgui::Slider::new(im_str!("Effect"), std::ops::RangeInclusive::new(0, 1))
                        .display_format(&ImString::from(effect_typename))
                        .build(&ui, &mut effect_type)
                    {
                        *effect = if effect_type == 0 {
                            Effect::None
                        } else {
                            Effect::Freeze
                        };
                    }
                }
                Action::Motion(motion) => {
                    let mut current_motion_position = match motion {
                        Motion::Stop => 0,
                        Motion::GoStraight { .. } => 1,
                        Motion::JumpTo(_) => 2,
                        Motion::Roam { .. } => 3,
                        Motion::Swap { .. } => 4,
                        Motion::Target { .. } => 5,
                        Motion::Accelerate { .. } => 6,
                    };
                    let motion_names = [
                        im_str!("Stop"),
                        im_str!("Go Straight"),
                        im_str!("Jump To"),
                        im_str!("Roam"),
                        im_str!("Swap"),
                        im_str!("Target"),
                        im_str!("Accelerate"),
                    ];
                    if imgui::ComboBox::new(im_str!("Motion")).build_simple_string(
                        &ui,
                        &mut current_motion_position,
                        &motion_names,
                    ) {
                        *motion = match current_motion_position {
                            0 => Motion::Stop,
                            1 => Motion::GoStraight {
                                direction: MovementDirection::Angle(Angle::Current),
                                speed: Speed::Category(SpeedCategory::Normal),
                            },
                            2 => Motion::JumpTo(JumpLocation::Mouse),
                            3 => Motion::Roam {
                                movement_type: MovementType::Wiggle,
                                area: AABB::new(0.0, 0.0, 0.0, 0.0),
                                speed: Speed::Category(SpeedCategory::Normal),
                            },
                            4 => Motion::Swap {
                                name: object_names[0].clone(),
                            },
                            5 => Motion::Target {
                                target: Target::Mouse,
                                target_type: TargetType::StopWhenReached,
                                offset: Vec2::zero(),
                                speed: Speed::Category(SpeedCategory::Normal),
                            },
                            6 => Motion::Accelerate {
                                direction: MovementDirection::Angle(Angle::Current),
                                speed: Speed::Category(SpeedCategory::Normal),
                            },
                            _ => unreachable!(),
                        };
                    }

                    match motion {
                        Motion::GoStraight { direction, speed }
                        | Motion::Accelerate { direction, speed } => {
                            let direction_types = [
                                im_str!("Current Angle"),
                                im_str!("Chosen Angle"),
                                im_str!("Random Angle"),
                                im_str!("Direction"),
                            ];
                            let mut current_direction_position = match direction {
                                MovementDirection::Angle(Angle::Current) => 0,
                                MovementDirection::Angle(Angle::Degrees(_)) => 1,
                                MovementDirection::Angle(Angle::Random { .. }) => 2,
                                MovementDirection::Direction { .. } => 3,
                            };
                            if imgui::ComboBox::new(im_str!("Movement Direction"))
                                .build_simple_string(
                                    &ui,
                                    &mut current_direction_position,
                                    &direction_types,
                                )
                            {
                                *direction = match current_direction_position {
                                    0 => MovementDirection::Angle(Angle::Current),
                                    1 => MovementDirection::Angle(Angle::Degrees(0.0)),
                                    2 => MovementDirection::Angle(Angle::Random {
                                        min: 0.0,
                                        max: 360.0,
                                    }),
                                    3 => MovementDirection::Direction {
                                        possible_directions: HashSet::new(),
                                    },
                                    _ => unreachable!(),
                                };
                            }
                            match direction {
                                MovementDirection::Angle(Angle::Degrees(angle)) => {
                                    ui.input_float(im_str!("Angle"), angle).build();
                                }
                                MovementDirection::Angle(Angle::Random { min, max }) => {
                                    ui.input_float(im_str!("Min Angle"), min).build();
                                    ui.input_float(im_str!("Max Angle"), max).build();
                                }
                                MovementDirection::Direction {
                                    possible_directions,
                                } => {
                                    fn direction_checkbox(
                                        ui: &imgui::Ui,
                                        possible_directions: &mut HashSet<CompassDirection>,
                                        label: &imgui::ImStr,
                                        direction: CompassDirection,
                                    ) {
                                        let mut checked = possible_directions.contains(&direction);
                                        if imgui::Ui::checkbox(&ui, label, &mut checked) {
                                            if checked {
                                                possible_directions.insert(direction);
                                            } else {
                                                possible_directions.remove(&direction);
                                            }
                                        }
                                    }
                                    ui.text("Random directions:");
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Up"),
                                        CompassDirection::Up,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Up Right"),
                                        CompassDirection::UpRight,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Right"),
                                        CompassDirection::Right,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Down Right"),
                                        CompassDirection::DownRight,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Down"),
                                        CompassDirection::Down,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Down Left"),
                                        CompassDirection::DownLeft,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Left"),
                                        CompassDirection::Left,
                                    );
                                    direction_checkbox(
                                        &ui,
                                        possible_directions,
                                        im_str!("Up Left"),
                                        CompassDirection::UpLeft,
                                    );
                                }
                                _ => {}
                            }
                            let speeds = [
                                im_str!("Very Slow"),
                                im_str!("Slow"),
                                im_str!("Normal"),
                                im_str!("Fast"),
                                im_str!("Very Fast"),
                                im_str!("Value"),
                            ];
                            let mut current_speed = match speed {
                                Speed::Category(SpeedCategory::VerySlow) => 0,
                                Speed::Category(SpeedCategory::Slow) => 1,
                                Speed::Category(SpeedCategory::Normal) => 2,
                                Speed::Category(SpeedCategory::Fast) => 3,
                                Speed::Category(SpeedCategory::VeryFast) => 4,
                                Speed::Value(_) => 5,
                            };

                            if imgui::ComboBox::new(im_str!("Speed")).build_simple_string(
                                &ui,
                                &mut current_speed,
                                &speeds,
                            ) {
                                *speed = match current_speed {
                                    0 => Speed::Category(SpeedCategory::VerySlow),
                                    1 => Speed::Category(SpeedCategory::Slow),
                                    2 => Speed::Category(SpeedCategory::Normal),
                                    3 => Speed::Category(SpeedCategory::Fast),
                                    4 => Speed::Category(SpeedCategory::VeryFast),
                                    5 => Speed::Value(0.0),
                                    _ => unreachable!(),
                                };
                            }
                            match speed {
                                Speed::Value(value) => {
                                    ui.input_float(im_str!("Speed"), value).build();
                                }
                                _ => {}
                            }
                        }
                        Motion::JumpTo(jump_location) => {
                            let jump_types = [
                                im_str!("Point"),
                                im_str!("Area"),
                                im_str!("Mouse"),
                                im_str!("Object"),
                                im_str!("Relative"),
                                im_str!("Clamp Position"),
                            ];
                            let mut current_jump_position = match jump_location {
                                JumpLocation::Point(_) => 0,
                                JumpLocation::Area(_) => 1,
                                JumpLocation::Mouse => 2,
                                JumpLocation::Object { .. } => 3,
                                JumpLocation::Relative { .. } => 4,
                                JumpLocation::ClampPosition { .. } => 5,
                            };
                            if imgui::ComboBox::new(im_str!("Jump To")).build_simple_string(
                                &ui,
                                &mut current_jump_position,
                                &jump_types,
                            ) {
                                *jump_location = match current_jump_position {
                                    0 => JumpLocation::Point(Vec2::zero()),
                                    1 => JumpLocation::Area(AABB::new(0.0, 0.0, 1600.0, 900.0)),
                                    2 => JumpLocation::Mouse,
                                    3 => JumpLocation::Object {
                                        name: object_names[0].clone(),
                                    },
                                    4 => JumpLocation::Relative {
                                        to: RelativeTo::CurrentAngle,
                                        distance: Vec2::zero(),
                                    },
                                    5 => JumpLocation::ClampPosition {
                                        area: AABB::new(0.0, 0.0, 1600.0, 900.0),
                                    },
                                    _ => unreachable!(),
                                };
                            }

                            match jump_location {
                                JumpLocation::Area(area) => {
                                    ui.input_float(im_str!("Area Min X"), &mut area.min.x)
                                        .build();
                                    ui.input_float(im_str!("Area Min Y"), &mut area.min.y)
                                        .build();
                                    ui.input_float(im_str!("Area Max X"), &mut area.max.x)
                                        .build();
                                    ui.input_float(im_str!("Area Max Y"), &mut area.max.y)
                                        .build();
                                }
                                JumpLocation::Object { name } => {
                                    let mut current_object = object_names
                                        .iter()
                                        .position(|obj_name| obj_name == name)
                                        .unwrap();
                                    let keys: Vec<ImString> = object_names
                                        .iter()
                                        .map(|name| ImString::from(name.clone()))
                                        .collect();
                                    let combo_keys: Vec<_> = keys.iter().collect();
                                    if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                                        &ui,
                                        &mut current_object,
                                        &combo_keys,
                                    ) {
                                        *name = object_names[current_object].clone();
                                    }
                                }
                                JumpLocation::Point(point) => {
                                    ui.input_float(im_str!("X"), &mut point.x).build();
                                    ui.input_float(im_str!("Y"), &mut point.y).build();
                                }
                                JumpLocation::Relative { to, distance } => {
                                    if ui.radio_button_bool(
                                        im_str!("Relative To Current Angle"),
                                        *to == RelativeTo::CurrentAngle,
                                    ) {
                                        *to = RelativeTo::CurrentAngle;
                                    }
                                    if ui.radio_button_bool(
                                        im_str!("Relative To Current Position"),
                                        *to == RelativeTo::CurrentPosition,
                                    ) {
                                        *to = RelativeTo::CurrentPosition;
                                    }
                                    ui.input_float(im_str!("Distance X"), &mut distance.x)
                                        .build();
                                    ui.input_float(im_str!("Distance Y"), &mut distance.y)
                                        .build();
                                }
                                JumpLocation::ClampPosition { area } => {
                                    ui.input_float(im_str!("Area Min X"), &mut area.min.x)
                                        .build();
                                    ui.input_float(im_str!("Area Min Y"), &mut area.min.y)
                                        .build();
                                    ui.input_float(im_str!("Area Max X"), &mut area.max.x)
                                        .build();
                                    ui.input_float(im_str!("Area Max Y"), &mut area.max.y)
                                        .build();
                                }
                                _ => {}
                            }
                        }
                        Motion::Roam {
                            movement_type,
                            area,
                            speed,
                        } => {
                            let movement_types = [
                                im_str!("Wiggle"),
                                im_str!("Insect"),
                                im_str!("Reflect"),
                                im_str!("Bounce"),
                            ];
                            let mut current_movement_position = match movement_type {
                                MovementType::Wiggle => 0,
                                MovementType::Insect => 1,
                                MovementType::Reflect { .. } => 2,
                                MovementType::Bounce { .. } => 3,
                            };

                            if imgui::ComboBox::new(im_str!("Movement Type")).build_simple_string(
                                &ui,
                                &mut current_movement_position,
                                &movement_types,
                            ) {
                                *movement_type = match current_movement_position {
                                    0 => MovementType::Wiggle,
                                    1 => MovementType::Insect,
                                    2 => MovementType::Reflect {
                                        initial_direction: MovementDirection::Angle(Angle::Current),
                                        movement_handling: MovementHandling::Anywhere,
                                    },
                                    3 => MovementType::Bounce {
                                        initial_direction: None,
                                    },
                                    _ => unreachable!(),
                                };
                            }

                            match movement_type {
                                MovementType::Reflect {
                                    initial_direction,
                                    movement_handling,
                                } => {
                                    let movement_handling_types =
                                        [im_str!("Anywhere"), im_str!("Try Not To Overlap")];

                                    let mut current_movement = *movement_handling as usize;

                                    if imgui::ComboBox::new(im_str!("Movement Handling"))
                                        .build_simple_string(
                                            &ui,
                                            &mut current_movement,
                                            &movement_handling_types,
                                        )
                                    {
                                        *movement_handling = match current_movement {
                                            0 => MovementHandling::Anywhere,
                                            1 => MovementHandling::TryNotToOverlap,
                                            _ => MovementHandling::Anywhere,
                                        };
                                    }

                                    match initial_direction {
                                        MovementDirection::Angle(Angle::Degrees(angle)) => {
                                            ui.input_float(im_str!("Angle"), angle).build();
                                        }
                                        MovementDirection::Angle(Angle::Random { min, max }) => {
                                            ui.input_float(im_str!("Min Angle"), min).build();
                                            ui.input_float(im_str!("Max Angle"), max).build();
                                        }
                                        MovementDirection::Direction {
                                            possible_directions,
                                        } => {
                                            fn direction_checkbox(
                                                ui: &imgui::Ui,
                                                possible_directions: &mut HashSet<CompassDirection>,
                                                label: &imgui::ImStr,
                                                direction: CompassDirection,
                                            ) {
                                                let mut checked =
                                                    possible_directions.contains(&direction);
                                                if imgui::Ui::checkbox(&ui, label, &mut checked) {
                                                    if checked {
                                                        possible_directions.insert(direction);
                                                    } else {
                                                        possible_directions.remove(&direction);
                                                    }
                                                }
                                            }
                                            ui.text("Random directions:");
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Up"),
                                                CompassDirection::Up,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Up Right"),
                                                CompassDirection::UpRight,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Right"),
                                                CompassDirection::Right,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Down Right"),
                                                CompassDirection::DownRight,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Down"),
                                                CompassDirection::Down,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Down Left"),
                                                CompassDirection::DownLeft,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Left"),
                                                CompassDirection::Left,
                                            );
                                            direction_checkbox(
                                                &ui,
                                                possible_directions,
                                                im_str!("Up Left"),
                                                CompassDirection::UpLeft,
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                                MovementType::Bounce { initial_direction } => {
                                    if ui.radio_button_bool(
                                        im_str!("Initial Direction Left"),
                                        *initial_direction == Some(BounceDirection::Left),
                                    ) {
                                        *initial_direction = Some(BounceDirection::Left);
                                    }
                                    if ui.radio_button_bool(
                                        im_str!("Initial Direction Right"),
                                        *initial_direction == Some(BounceDirection::Right),
                                    ) {
                                        *initial_direction = Some(BounceDirection::Right);
                                    }
                                    if ui.radio_button_bool(
                                        im_str!("No Initial Direction"),
                                        *initial_direction == None,
                                    ) {
                                        *initial_direction = None;
                                    }
                                }
                                _ => {}
                            }
                            ui.input_float(im_str!("Area Min X"), &mut area.min.x)
                                .build();
                            ui.input_float(im_str!("Area Min Y"), &mut area.min.y)
                                .build();
                            ui.input_float(im_str!("Area Max X"), &mut area.max.x)
                                .build();
                            ui.input_float(im_str!("Area Max Y"), &mut area.max.y)
                                .build();

                            let speeds = [
                                im_str!("Very Slow"),
                                im_str!("Slow"),
                                im_str!("Normal"),
                                im_str!("Fast"),
                                im_str!("Very Fast"),
                                im_str!("Value"),
                            ];
                            let mut current_speed = match speed {
                                Speed::Category(SpeedCategory::VerySlow) => 0,
                                Speed::Category(SpeedCategory::Slow) => 1,
                                Speed::Category(SpeedCategory::Normal) => 2,
                                Speed::Category(SpeedCategory::Fast) => 3,
                                Speed::Category(SpeedCategory::VeryFast) => 4,
                                Speed::Value(_) => 5,
                            };

                            if imgui::ComboBox::new(im_str!("Speed")).build_simple_string(
                                &ui,
                                &mut current_speed,
                                &speeds,
                            ) {
                                *speed = match current_speed {
                                    0 => Speed::Category(SpeedCategory::VerySlow),
                                    1 => Speed::Category(SpeedCategory::Slow),
                                    2 => Speed::Category(SpeedCategory::Normal),
                                    3 => Speed::Category(SpeedCategory::Fast),
                                    4 => Speed::Category(SpeedCategory::VeryFast),
                                    5 => Speed::Value(0.0),
                                    _ => unreachable!(),
                                };
                            }
                            match speed {
                                Speed::Value(value) => {
                                    ui.input_float(im_str!("Speed"), value).build();
                                }
                                _ => {}
                            }
                        }
                        Motion::Swap { name } => {
                            let mut current_object = object_names
                                .iter()
                                .position(|obj_name| obj_name == name)
                                .unwrap();
                            let keys: Vec<ImString> = object_names
                                .iter()
                                .map(|name| ImString::from(name.clone()))
                                .collect();
                            let combo_keys: Vec<_> = keys.iter().collect();
                            if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                                &ui,
                                &mut current_object,
                                &combo_keys,
                            ) {
                                *name = object_names[current_object].clone();
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }

            if ui.small_button(im_str!("Back")) {
                *instruction_mode = InstructionMode::Edit;
            }
        }
    }
}

fn load_textures(
    asset_files: &mut AssetFiles,
    images: &mut Images,
    image_filenames: Vec<(WeeResult<String>, String)>,
) -> Option<String> {
    let mut first_key = None;
    for (key, path) in &image_filenames {
        match key {
            Ok(key) => {
                let texture = Texture::from_file(path);
                match texture {
                    Ok(texture) => {
                        images.insert(key.clone(), texture);
                        let filename = Path::new(path)
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string();
                        asset_files.images.insert(key.clone(), filename);
                        first_key = Some(key.clone());
                    }
                    Err(error) => {
                        log::error!("Could not add image with filename {}", path);
                        log::error!("{}", error);
                    }
                }
            }
            Err(error) => {
                log::error!("Could not add image with filename {}", path);
                log::error!("{}", error);
            }
        }
    }
    first_key
}

fn get_main_filename_part(path: &Path) -> WeeResult<String> {
    let parent = path.parent().ok_or("Could not find filename parent")?;
    let name = path
        .strip_prefix(parent)?
        .file_stem()
        .ok_or("Could not get file stem")?
        .to_str()
        .ok_or("Could not convert main filename part to string")?
        .to_string();
    Ok(name)
}
fn choose_image_from_files<P: AsRef<Path>>(
    asset_files: &mut AssetFiles,
    images: &mut Images,
    images_path: P,
) -> Option<String> {
    let result = nfd::open_file_multiple_dialog(None, images_path.as_ref().to_str()).unwrap();

    match result {
        Response::Okay(_) => {
            unreachable!();
        }
        Response::OkayMultiple(files) => {
            log::info!("Files {:?}", files);
            let image_filenames: Vec<(WeeResult<String>, String)> = files
                .into_iter()
                .map(|file_path| {
                    let path = std::path::Path::new(&file_path);
                    let name = get_main_filename_part(&path);
                    let file_path = path
                        .strip_prefix(std::env::current_dir().unwrap().as_path())
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string();
                    (name, file_path)
                })
                .collect();

            load_textures(asset_files, images, image_filenames)
        }
        _ => None,
    }
}

fn properties_window(
    ui: &imgui::Ui,
    game: &mut GameData,
    index: usize,
    images: &mut Images,
    filename: &Option<String>,
) {
    let object = game.objects.get_mut(index).unwrap();
    let mut sprite_type = if let Sprite::Image { .. } = &object.sprite {
        0
    } else {
        1
    };
    let sprite_typename = if sprite_type == 0 {
        "Image".to_string()
    } else {
        "Colour".to_string()
    };
    if imgui::Slider::new(im_str!("Sprite"), std::ops::RangeInclusive::new(0, 1))
        .display_format(&ImString::from(sprite_typename))
        .build(&ui, &mut sprite_type)
    {
        object.sprite = if sprite_type == 0 {
            Sprite::Image {
                name: images.keys().next().unwrap().clone(),
            }
        } else {
            Sprite::Colour(Colour::black())
        };
    }

    match &mut object.sprite {
        Sprite::Image { name: image_name } => {
            // TODO: Tidy up
            let mut current_image = images.keys().position(|k| k == image_name).unwrap_or(0);
            let path = match filename {
                Some(filename) => Path::new(filename).parent().unwrap().join("images"),
                None => Path::new("games").to_owned(),
            };

            if images.is_empty() {
                if ui.button(im_str!("Add a New Image"), NORMAL_BUTTON) {
                    let first_key = choose_image_from_files(&mut game.asset_files, images, path);
                    match first_key {
                        Some(key) => {
                            object.sprite = Sprite::Image { name: key.clone() };
                            object.size.width = images[&key].width as f32;
                            object.size.height = images[&key].height as f32;
                        }
                        None => {
                            log::error!("None of the new images loaded correctly");
                        }
                    }
                }
            } else {
                let mut keys: Vec<ImString> =
                    images.keys().map(|k| ImString::from(k.clone())).collect();

                keys.push(ImString::new("Add a New Image"));

                let image_names: Vec<&ImString> = keys.iter().collect();

                if imgui::ComboBox::new(im_str!("Image")).build_simple_string(
                    &ui,
                    &mut current_image,
                    &image_names,
                ) {
                    if current_image == image_names.len() - 1 {
                        let first_key =
                            choose_image_from_files(&mut game.asset_files, images, path);
                        match first_key {
                            Some(key) => {
                                object.sprite = Sprite::Image { name: key.clone() };
                                object.size.width = images[&key].width as f32;
                                object.size.height = images[&key].height as f32;
                            }
                            None => {
                                log::error!("None of the new images loaded correctly");
                            }
                        }
                    } else {
                        match images.keys().nth(current_image) {
                            Some(image) => {
                                object.sprite = Sprite::Image {
                                    name: image.clone(),
                                };
                            }
                            None => {
                                log::error!("Could not set image to index {}", current_image);
                            }
                        }
                    }
                }
            };
        }
        Sprite::Colour(colour) => {
            let mut colour_array = [colour.r, colour.g, colour.b, colour.a];
            imgui::ColorEdit::new(im_str!("Colour"), &mut colour_array)
                .alpha(true)
                .build(ui);
            *colour = Colour::rgba(
                colour_array[0],
                colour_array[1],
                colour_array[2],
                colour_array[3],
            );
        }
    };

    ui.input_float(im_str!("Starting X"), &mut object.position.x)
        .build();
    ui.input_float(im_str!("Starting Y"), &mut object.position.y)
        .build();
    ui.input_float(im_str!("Width"), &mut object.size.width)
        .build();
    ui.input_float(im_str!("Height"), &mut object.size.height)
        .build();

    let mut radians = object.angle.to_radians();
    imgui::AngleSlider::new(im_str!("Angle"))
        .min_degrees(-360.0)
        .max_degrees(360.0)
        .build(ui, &mut radians);
    object.angle = radians.to_degrees();

    object.origin = match object.origin {
        Some(mut origin) => {
            ui.input_float(im_str!("Origin X"), &mut origin.x).build();
            ui.input_float(im_str!("Origin Y"), &mut origin.y).build();
            Some(origin)
        }
        None => {
            let mut origin = object.origin();
            let changed = ui.input_float(im_str!("Origin X"), &mut origin.x).build()
                || ui.input_float(im_str!("Origin Y"), &mut origin.y).build();
            if changed {
                Some(origin)
            } else {
                None
            }
        }
    };

    object.collision_area = match object.collision_area {
        Some(mut area) => {
            ui.input_float(im_str!("Collision Min X"), &mut area.min.x)
                .build();
            ui.input_float(im_str!("Collision Min Y"), &mut area.min.y)
                .build();
            ui.input_float(im_str!("Collision Max X"), &mut area.max.x)
                .build();
            ui.input_float(im_str!("Collision Max Y"), &mut area.max.y)
                .build();
            Some(area)
        }
        None => {
            let mut area = wee_common::AABB::new(
                0.0,
                0.0,
                object.size.width as f32,
                object.size.height as f32,
            );
            let changed = ui
                .input_float(im_str!("Collision Min X"), &mut area.min.x)
                .build()
                || ui
                    .input_float(im_str!("Collision Min Y"), &mut area.min.y)
                    .build()
                || ui
                    .input_float(im_str!("Collision Max X"), &mut area.max.x)
                    .build()
                || ui
                    .input_float(im_str!("Collision Max Y"), &mut area.max.y)
                    .build();
            if changed {
                Some(area)
            } else {
                None
            }
        }
    };

    if ui.radio_button_bool(im_str!("Flip Horizontal"), object.flip.horizontal) {
        object.flip.horizontal = !object.flip.horizontal;
    }
    ui.same_line(0.0);
    if ui.radio_button_bool(im_str!("Flip Vertical"), object.flip.vertical) {
        object.flip.vertical = !object.flip.vertical;
    }

    let switch = object.switch == Switch::On;
    if ui.radio_button_bool(im_str!("Switch"), switch) {
        object.switch = if switch { Switch::Off } else { Switch::On };
    }

    let mut change_layer = object.layer as i32;
    ui.input_int(im_str!("Layer"), &mut change_layer).build();
    object.layer = change_layer.max(0).min(255) as u8;
}

fn rename_across_objects(objects: &mut Vec<SerialiseObject>, old_name: &str, new_name: &str) {
    for obj in objects.iter_mut() {
        rename_object(obj, old_name, new_name);
    }
}

fn rename_object(object: &mut SerialiseObject, old_name: &str, new_name: &str) {
    let rename = |other_name: &mut String| {
        if *other_name == old_name {
            *other_name = new_name.to_string();
        }
    };
    for instruction in object.instructions.iter_mut() {
        for trigger in instruction.triggers.iter_mut() {
            match trigger {
                Trigger::Collision(CollisionWith::Object { name }) => {
                    rename(name);
                }
                Trigger::Input(Input::Mouse { over, .. }) => {
                    if let MouseOver::Object { name } = over {
                        rename(name);
                    }
                }
                Trigger::CheckProperty { name, .. } => {
                    rename(name);
                }
                _ => {}
            }
        }
        rename_actions(&mut instruction.actions, &old_name, &new_name);
    }
}

fn rename_actions(actions: &mut Vec<Action>, old_name: &str, new_name: &str) {
    let rename = |other_name: &mut String| {
        if *other_name == old_name {
            *other_name = new_name.to_string();
        }
    };
    for action in actions.iter_mut() {
        match action {
            Action::SetProperty(PropertySetter::Angle(angle_setter)) => {
                if let AngleSetter::Match { name } = angle_setter {
                    rename(name);
                }
            }
            Action::Motion(motion) => match motion {
                Motion::JumpTo(jump_location) => {
                    if let JumpLocation::Object { name } = jump_location {
                        rename(name)
                    }
                }
                Motion::Swap { name } => rename(name),
                Motion::Target { target, .. } => {
                    if let Target::Object { name } = target {
                        rename(name)
                    }
                }
                _ => {}
            },
            Action::Random { random_actions } => {
                rename_actions(random_actions, old_name, new_name);
            }
            _ => {}
        }
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

                let imgui_frame = imgui.prepare_frame(&renderer, &event_pump, &mut last_frame);
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

                imgui::Window::new(im_str!("Main Window"))
                    .size([500.0, 600.0], imgui::Condition::FirstUseEver)
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
                    .size([500.0, 600.0], imgui::Condition::FirstUseEver)
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
                                    let mut text = imgui::ImString::from("Test".to_string());
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

                        right_window(
                            ui,
                            &mut game,
                            &mut images,
                            &filename,
                            &selected_index,
                            &mut instruction_mode,
                            &mut instruction_index,
                            &mut instruction_focus,
                        );
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

                imgui_frame.render(renderer);
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
        const INITIAL_WINDOW_WIDTH: u32 = 1600; //1200;
        const INITIAL_WINDOW_HEIGHT: u32 = 900; //675;

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
