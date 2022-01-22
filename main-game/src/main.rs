#![windows_subsystem = "windows"]

use macroquad::logging as log;
use macroquad::prelude::*;
use macroquad::texture;
use macroquad::{
    audio::{self, PlaySoundParams, Sound},
    camera::{self, Camera2D},
    experimental::coroutines::{start_coroutine, Coroutine},
    math::Rect as MacroRect,
    window,
};

use futures::future::join_all;
use std::{
    collections::{HashMap, HashSet},
    default::Default,
    path::Path,
    str,
};
#[cfg(not(target_arch = "wasm32"))]
use walkdir::WalkDir;

use serde::Deserialize;

use wee_common::{Vec2 as WeeVec2, *};

use wee::*;

const PROJECTION_WIDTH: f32 = 1600.0;
const PROJECTION_HEIGHT: f32 = 900.0;
const WIDE_RATIO: f32 = PROJECTION_WIDTH / PROJECTION_HEIGHT + 0.0001;
const TALL_RATIO: f32 = PROJECTION_WIDTH / PROJECTION_HEIGHT - 0.0001;

const DEFAULT_DIFFICULTY: u32 = 1;
const DEFAULT_PLAYBACK_RATE: f32 = 1.0;
const MAX_LIVES: i32 = 4;
const INITIAL_PLAYBACK_RATE: f32 = 1.0;
const PLAYBACK_RATE_INCREASE: f32 = 0.1;
const PLAYBACK_RATE_MAX: f32 = 2.0;
const UP_TO_DIFFICULTY_TWO: i32 = 20;
const UP_TO_DIFFICULTY_THREE: i32 = 40;
const BOSS_GAME_INTERVAL: i32 = 15;
const INCREASE_SPEED_AFTER_GAMES: i32 = 5;
const VOLUME: f32 = 0.5;

async fn load_images<P: AsRef<Path>>(
    image_files: &HashMap<String, String>,
    base_path: P,
) -> WeeResult<Images> {
    log::debug!("Start loading images");
    let mut paths = Vec::new();
    let mut images = Images::new();
    let mut loading_images = Vec::new();

    let base_path = base_path.as_ref().join("images");

    for path in image_files.values() {
        let path = base_path.join(path);

        let path = path.to_str().unwrap().to_string();
        paths.push(path);
    }

    for path in &paths {
        loading_images.push(texture::load_texture(path));
    }

    let textures: Vec<_> = join_all(loading_images).await;

    for (key, texture) in image_files.keys().zip(textures) {
        let texture = texture?;
        texture.set_filter(macroquad::texture::FilterMode::Nearest);
        images.insert(key.to_string(), texture);
    }

    Ok(images)
}

async fn load_sounds(
    sound_files: &HashMap<String, String>,
    base_path: impl AsRef<Path>,
) -> WeeResult<Sounds> {
    log::debug!("Start loading sounds");
    let base_path = base_path.as_ref().join("audio");
    let mut sounds = Sounds::new();

    for (key, filename) in sound_files {
        let path = base_path.join(&filename);

        let sound = macroquad::audio::load_sound(&path.to_str().unwrap()).await?;

        sounds.insert(key.to_string(), sound);
    }
    Ok(sounds)
}

#[derive(Clone)]
struct Music {
    data: Sound,
    looped: bool,
}

async fn load_music(
    music_file: &Option<SerialiseMusic>,
    base_path: impl AsRef<Path>,
) -> WeeResult<Option<Music>> {
    log::debug!("Start loading music");
    let base_path = base_path.as_ref().join("audio");

    if let Some(music_info) = music_file {
        let path = base_path.join(&music_info.filename);

        let sound = macroquad::audio::load_sound(&path.to_str().unwrap()).await?;

        Ok(Some(Music {
            data: sound,
            looped: music_info.looped,
        }))
    } else {
        Ok(None)
    }
}

pub trait MusicPlayer {
    fn play(&self, playback_rate: f32, volume: f32);

    fn stop(&self);

    fn pause(&self) -> bool;
    fn resume(&self);
}

impl MusicPlayer for Option<Music> {
    fn play(&self, playback_rate: f32, volume: f32) {
        if let Some(music) = self {
            macroquad::audio::play_sound(
                music.data,
                PlaySoundParams {
                    looped: music.looped,
                    volume,
                    speed: playback_rate,
                },
            );
        }
    }

    fn stop(&self) {
        if let Some(music) = self {
            macroquad::audio::stop_sound(music.data);
        }
    }

    fn pause(&self) -> bool {
        if let Some(music) = self {
            macroquad::audio::pause_sound(music.data)
        } else {
            false
        }
    }

    fn resume(&self) {
        if let Some(music) = self {
            macroquad::audio::resume_sound(music.data);
        }
    }
}

impl Drop for Music {
    fn drop(&mut self) {
        macroquad::audio::stop_sound(self.data);
    }
}

async fn load_fonts(
    font_files: &HashMap<String, FontLoadInfo>,
    base_path: impl AsRef<Path>,
) -> WeeResult<Fonts> {
    log::debug!("Start loading fonts");
    let base_path = base_path.as_ref().join("fonts");
    let mut fonts = Fonts::new();

    for (key, font_info) in font_files {
        let path = base_path.join(&font_info.filename);

        let font = macroquad::text::load_ttf_font(path.to_str().unwrap()).await?;
        fonts.insert(key.to_string(), (font, font_info.size as u16));
    }
    Ok(fonts)
}

type Images = HashMap<String, Texture2D>;
type Fonts = HashMap<String, (Font, u16)>;
type Sounds = HashMap<String, Sound>;

fn json_from_str<'a, T: Deserialize<'a>>(text: &'a str) -> WeeResult<T> {
    match serde_json::from_str(text) {
        Ok(data) => Ok(data),
        Err(error) => Err(Box::new(error)),
    }
}

pub async fn load_game_data(filename: impl AsRef<Path>) -> WeeResult<GameData> {
    let json_string = macroquad::file::load_string(&filename.as_ref().to_string_lossy()).await?;

    json_from_str(&json_string)
}

#[derive(Clone)]
struct LoadedGameData {
    data: GameData,
    assets: Assets,
}

impl LoadedGameData {
    async fn load(filename: impl AsRef<Path>) -> WeeResult<LoadedGameData> {
        log::debug!("Loading game data");
        let game_data = load_game_data(&filename).await?;
        let base_path = filename.as_ref().parent().unwrap();
        let data = LoadedGameData {
            assets: Assets::load(&game_data.asset_files, base_path).await?,
            data: game_data,
        };
        Ok(data)
    }
}

#[derive(Clone)]
struct Assets {
    images: Images,
    music: Option<Music>,
    sounds: Sounds,
    fonts: Fonts,
}

impl Assets {
    async fn load(asset_files: &AssetFiles, base_path: impl AsRef<Path>) -> WeeResult<Assets> {
        log::debug!("Loading assets");
        let assets = Assets {
            images: load_images(&asset_files.images, &base_path).await?,
            music: load_music(&asset_files.music, &base_path).await?,
            sounds: load_sounds(&asset_files.audio, &base_path).await?,
            fonts: load_fonts(&asset_files.fonts, &base_path).await?,
        };
        Ok(assets)
    }

    fn stop_sounds(&self) {
        self.music.stop();

        for sound in self.sounds.values() {
            audio::stop_sound(*sound);
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct LastGame {
    has_won: bool,
    was_life_gained: bool,
}

#[derive(Debug, Copy, Clone)]
struct Progress {
    playback_rate: f32,
    score: i32,
    lives: i32,
    difficulty: u32,
    last_game: Option<LastGame>,
    boss_playback_rate: f32,
}

impl Progress {
    fn new() -> Progress {
        Progress {
            playback_rate: INITIAL_PLAYBACK_RATE,
            score: 0,
            lives: MAX_LIVES,
            difficulty: DEFAULT_DIFFICULTY,
            last_game: None,
            boss_playback_rate: INITIAL_PLAYBACK_RATE,
        }
    }

    fn update(&mut self, has_won: bool, is_boss_game: bool) {
        self.score += 1;
        if self.score % INCREASE_SPEED_AFTER_GAMES == 0 {
            self.playback_rate += PLAYBACK_RATE_INCREASE;
        }
        if self.score >= UP_TO_DIFFICULTY_THREE {
            self.difficulty = 3;
        } else if self.score >= UP_TO_DIFFICULTY_TWO {
            self.difficulty = 2;
        }
        let cap_playback_rate = |playback_rate: f32| playback_rate.min(PLAYBACK_RATE_MAX);
        self.playback_rate = cap_playback_rate(self.playback_rate);

        if is_boss_game {
            self.boss_playback_rate += PLAYBACK_RATE_INCREASE;
            self.boss_playback_rate = cap_playback_rate(self.boss_playback_rate);
        }

        if !has_won {
            self.lives -= 1;
        }

        let was_life_gained = is_boss_game && has_won && self.lives < MAX_LIVES;
        if was_life_gained {
            self.lives += 1;
        }
        self.last_game = Some(LastGame {
            has_won,
            was_life_gained,
        });
    }
}

// Adapted from draw_rectangle/draw_texture_ex in macroquad
fn draw_rectangle_ex(
    color: Color,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rotation: f32,
    pivot: Option<glam::f32::Vec2>,
) {
    unsafe {
        let gl = macroquad::window::get_internal_gl().quad_gl;

        let pivot = pivot.unwrap_or_else(|| vec2(x + w / 2., y + h / 2.));
        let m = pivot;
        let p = [
            vec2(x, y) - pivot,
            vec2(x + w, y) - pivot,
            vec2(x + w, y + h) - pivot,
            vec2(x, y + h) - pivot,
        ];
        let r = rotation;
        let p = [
            vec2(
                p[0].x * r.cos() - p[0].y * r.sin(),
                p[0].x * r.sin() + p[0].y * r.cos(),
            ) + m,
            vec2(
                p[1].x * r.cos() - p[1].y * r.sin(),
                p[1].x * r.sin() + p[1].y * r.cos(),
            ) + m,
            vec2(
                p[2].x * r.cos() - p[2].y * r.sin(),
                p[2].x * r.sin() + p[2].y * r.cos(),
            ) + m,
            vec2(
                p[3].x * r.cos() - p[3].y * r.sin(),
                p[3].x * r.sin() + p[3].y * r.cos(),
            ) + m,
        ];
        #[rustfmt::skip]
        let vertices = [
            Vertex::new(p[0].x, p[0].y, 0.0,  0.0,  0.0, color),
            Vertex::new(p[1].x, p[1].y, 0.0, 1.0,  0.0, color),
            Vertex::new(p[2].x, p[2].y, 0.0, 1.0, 1.0, color),
            Vertex::new(p[3].x, p[3].y, 0.0,  0.0, 1.0, color),
        ];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];

        gl.texture(None);
        gl.draw_mode(DrawMode::Triangles);
        gl.geometry(&vertices, &indices);
    }
}

fn draw_game(
    game: &Game,
    images: &Images,
    fonts: &Fonts,
    intro_font: &Font,
    drawn_text: &HashMap<String, DrawnText>,
) {
    let screen_width = window::screen_width();
    let screen_height = window::screen_height();
    let ratio = screen_width / screen_height;
    let intended_ratio = PROJECTION_WIDTH / PROJECTION_HEIGHT;
    if ratio > WIDE_RATIO {
        {
            let scaled_projection_width = (PROJECTION_HEIGHT / screen_height) * screen_width;
            let camera_x = (scaled_projection_width - PROJECTION_WIDTH) / 2.0;
            let camera = Camera2D::from_display_rect(MacroRect::new(
                -camera_x,
                0.0,
                scaled_projection_width,
                PROJECTION_HEIGHT,
            ));
            camera::set_camera(&camera);
        }

        {
            let game_area_rendered_width = screen_height * intended_ratio;
            let blank_space = screen_width - game_area_rendered_width;
            let letterbox_width = blank_space / 2.0;
            unsafe {
                let gl = window::get_internal_gl().quad_gl;
                gl.scissor(Some((
                    letterbox_width as i32,
                    0,
                    (screen_width - blank_space) as i32,
                    screen_height as i32,
                )));
            }
        }
    } else if ratio < TALL_RATIO {
        let scaled_projection_height = (PROJECTION_WIDTH / screen_width) * screen_height;
        let camera_y = (scaled_projection_height - PROJECTION_HEIGHT) / 2.0;
        let camera = macroquad::camera::Camera2D::from_display_rect(MacroRect::new(
            0.0,
            -camera_y,
            PROJECTION_WIDTH,
            scaled_projection_height,
        ));
        macroquad::camera::set_camera(&camera);

        let game_area_rendered_height = screen_width / intended_ratio;
        let blank_space = screen_height - game_area_rendered_height;
        let letterbox_height = blank_space / 2.0;
        unsafe {
            let gl = window::get_internal_gl().quad_gl;
            gl.scissor(Some((
                0,
                letterbox_height as i32,
                screen_width as i32,
                (screen_height - blank_space) as i32,
            )));
        }
    } else {
        let camera = macroquad::camera::Camera2D::from_display_rect(MacroRect::new(
            0.0,
            0.0,
            PROJECTION_WIDTH,
            PROJECTION_HEIGHT,
        ));
        macroquad::camera::set_camera(&camera);
    }
    clear_background(BLACK);
    macroquad::shapes::draw_rectangle(0.0, 0.0, PROJECTION_WIDTH, PROJECTION_HEIGHT, WHITE);
    // Draw background
    for part in &game.background {
        match &part.sprite {
            Sprite::Image { name } => {
                let params = macroquad::texture::DrawTextureParams {
                    dest_size: Some(macroquad::math::Vec2::new(
                        part.area.width(),
                        part.area.height(),
                    )),
                    source: None,
                    rotation: 0.0,
                    pivot: None,
                    flip_x: false,
                    flip_y: false,
                };
                draw_texture_ex(
                    images[name],
                    part.area.min.x,
                    part.area.min.y,
                    macroquad::color::WHITE,
                    params,
                );
            }
            Sprite::Colour(colour) => macroquad::shapes::draw_rectangle(
                part.area.min.x,
                part.area.min.y,
                part.area.max.x,
                part.area.max.y,
                macroquad::color::Color::new(colour.r, colour.g, colour.b, colour.a),
            ),
        }
    }

    // Draw Objects
    let mut layers: Vec<u8> = game.objects.values().map(|o| o.layer).collect();
    layers.sort_unstable();
    layers.dedup();
    layers.reverse();
    for layer in layers.into_iter() {
        for (key, object) in game.objects.iter() {
            if object.layer == layer {
                match &object.sprite {
                    Sprite::Image { name } => {
                        let origin = object.origin_in_world();
                        let origin = macroquad::math::Vec2::new(origin.x, origin.y);
                        let params = macroquad::texture::DrawTextureParams {
                            dest_size: Some(macroquad::math::Vec2::new(
                                object.size.width,
                                object.size.height,
                            )),
                            source: None,
                            rotation: object.angle.to_radians(),
                            pivot: Some(origin),
                            flip_x: object.flip.horizontal,
                            flip_y: object.flip.vertical,
                        };
                        draw_texture_ex(
                            images[name],
                            object.position.x - object.size.width / 2.0,
                            object.position.y - object.size.height / 2.0,
                            macroquad::color::WHITE,
                            params,
                        );
                    }
                    Sprite::Colour(colour) => {
                        let origin = object.origin_in_world();
                        let origin = macroquad::math::Vec2::new(origin.x, origin.y);
                        draw_rectangle_ex(
                            Color::new(colour.r, colour.g, colour.b, colour.a),
                            object.position.x - object.size.width / 2.0,
                            object.position.y - object.size.height / 2.0,
                            object.size.width,
                            object.size.height,
                            object.angle.to_radians(),
                            Some(origin),
                        );
                    }
                }

                if drawn_text.contains_key(key) {
                    let colour = drawn_text[key].colour;
                    let colour = Color::new(colour.r, colour.g, colour.b, colour.a);
                    let size = macroquad::text::measure_text(
                        &drawn_text[key].text,
                        Some(fonts[&drawn_text[key].font].0),
                        fonts[&drawn_text[key].font].1,
                        1.0,
                    );
                    let position = match drawn_text[key].justify {
                        JustifyText::Left => WeeVec2::new(
                            object.position.x - object.half_width(),
                            object.position.y + size.height / 2.0,
                        ),
                        JustifyText::Centre => WeeVec2::new(
                            object.position.x - size.width / 2.0,
                            object.position.y + size.height / 2.0,
                        ),
                    };
                    let params = macroquad::text::TextParams {
                        font: fonts[&drawn_text[key].font].0,
                        font_size: fonts[&drawn_text[key].font].1,
                        font_scale: 1.0,
                        font_scale_aspect: 1.0,
                        color: colour,
                    };
                    macroquad::text::draw_text_ex(
                        &drawn_text[key].text,
                        position.x,
                        position.y,
                        params,
                    );
                }
            }
        }
    }

    // Draw Intro Text
    const INTRO_TEXT_TIME: u32 = 60;
    if game.frames.ran < INTRO_TEXT_TIME {
        let colour = BLACK;
        let size = macroquad::text::measure_text(&game.intro_text, Some(*intro_font), 176, 1.01);
        let params = macroquad::text::TextParams {
            font: *intro_font,
            font_size: 178,
            font_scale: 1.01,
            font_scale_aspect: 1.0,
            color: colour,
        };
        macroquad::text::draw_text_ex(
            &game.intro_text,
            PROJECTION_WIDTH / 2.0 - size.width / 2.0,
            PROJECTION_HEIGHT / 2.0,
            params,
        );

        let colour = WHITE;
        let size = macroquad::text::measure_text(&game.intro_text, Some(*intro_font), 174, 1.0);
        let params = macroquad::text::TextParams {
            font: *intro_font,
            font_size: 174,
            font_scale: 1.0,
            font_scale_aspect: 1.0,
            color: colour,
        };
        macroquad::text::draw_text_ex(
            &game.intro_text,
            PROJECTION_WIDTH / 2.0 - size.width / 2.0,
            PROJECTION_HEIGHT / 2.0,
            params,
        );
    }
}

struct GameOutput {
    drawn_text: HashMap<String, DrawnText>,
    end_early: bool,
}

impl GameOutput {
    fn add_drawn_text(self, drawn_text: &mut HashMap<String, DrawnText>) -> GameOutput {
        drawn_text.extend(self.drawn_text.clone());
        self
    }

    fn should_end_early(self) -> bool {
        self.end_early
    }
}

// TODO: Temporary code to get around 144hz issue
static mut LAST_MOUSE_STATE: ButtonState = ButtonState::Up;

fn get_button_state() -> ButtonState {
    unsafe {
        LAST_MOUSE_STATE = if macroquad::input::is_mouse_button_down(MouseButton::Left) {
            if LAST_MOUSE_STATE == ButtonState::Up || LAST_MOUSE_STATE == ButtonState::Release {
                ButtonState::Press
            } else {
                ButtonState::Down
            }
        } else {
            if LAST_MOUSE_STATE == ButtonState::Down || LAST_MOUSE_STATE == ButtonState::Press {
                ButtonState::Release
            } else {
                ButtonState::Up
            }
        };
        LAST_MOUSE_STATE
    }
}

fn update_frame(
    game: &mut Game,
    assets: &Assets,
    playback_rate: f32,
    rng: &mut impl WeeRng,
) -> WeeResult<GameOutput> {
    let position = macroquad::input::mouse_position();
    let position = WeeVec2::new(position.0 as f32, position.1 as f32);

    let screen_width = window::screen_width();
    let screen_height = window::screen_height();

    let ratio = screen_width / screen_height;
    let intended_ratio = PROJECTION_WIDTH / PROJECTION_HEIGHT;

    let position = if ratio > WIDE_RATIO {
        let scaled_projection_width = (PROJECTION_HEIGHT / screen_height) * screen_width;
        let game_area_rendered_width = screen_height * intended_ratio;
        let blank_space = screen_width - game_area_rendered_width;
        let letterbox_width = blank_space / 2.0;
        WeeVec2::new(
            (position.x - letterbox_width) / screen_width * scaled_projection_width,
            position.y / screen_height * PROJECTION_HEIGHT,
        )
    } else if ratio < TALL_RATIO {
        let scaled_projection_height = (PROJECTION_WIDTH / screen_width) * screen_height;

        let game_area_rendered_height = screen_width / intended_ratio;
        let blank_space = screen_height - game_area_rendered_height;
        let letterbox_height = blank_space / 2.0;
        WeeVec2::new(
            position.x / screen_width * PROJECTION_WIDTH,
            (position.y - letterbox_height) / screen_height * scaled_projection_height,
        )
    } else {
        WeeVec2::new(
            position.x / screen_width as f32 * PROJECTION_WIDTH,
            position.y / screen_height as f32 * PROJECTION_HEIGHT,
        )
    };

    let mouse = Mouse {
        position,
        state: get_button_state(),
    };

    let world_actions = game.update_frame(mouse, rng)?;

    let mut drawn_text = HashMap::new();
    let mut end_early = false;

    for action in world_actions {
        match action {
            WorldAction::PlaySound { name } => {
                audio::play_sound(
                    assets.sounds[&name],
                    PlaySoundParams {
                        looped: false,
                        volume: VOLUME,
                        speed: playback_rate,
                    },
                );
            }
            WorldAction::StopMusic => {
                assets.music.stop();
            }
            WorldAction::EndEarly => {
                end_early = true;
            }
            WorldAction::DrawText { name, text } => {
                drawn_text.insert(name, text);
            }
        }
    }

    Ok(GameOutput {
        drawn_text,
        end_early,
    })
}

pub struct MacroRng {}

impl WeeRng for MacroRng {
    fn random_in_range(&mut self, min: f32, max: f32) -> f32 {
        rand::gen_range(min, max)
    }

    fn random_in_range_u32(&mut self, min: u32, max: u32) -> u32 {
        rand::gen_range(min, max)
    }

    fn random_in_slice<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
        if slice.is_empty() {
            None
        } else {
            Some(&slice[rand::gen_range(0, slice.len())])
        }
    }

    fn coin_flip(&mut self) -> bool {
        rand::gen_range(0, 2) == 1
    }
}

#[derive(Debug)]
struct GamesList {
    games: Vec<String>,
    bosses: Vec<String>,
    next: Vec<String>,
    directory: String,
}

impl GamesList {
    fn from_directory(all_games: &HashMap<String, GameData>, directory: String) -> GamesList {
        let mut games = Vec::new();
        let mut bosses = Vec::new();
        for (filename, game) in all_games {
            // TODO: Wiser way of doing this
            // TODO: Unpublished games are included in list games to play
            if Path::new(&filename).starts_with(&directory) && game.published {
                if game.game_type == GameType::Minigame {
                    games.push(filename.clone());
                } else if game.game_type == GameType::BossGame {
                    bosses.push(filename.clone());
                }
            }
        }

        GamesList {
            games,
            bosses,
            directory,
            next: Vec::new(),
        }
    }

    fn choose_game(&mut self) -> String {
        while !self.games.is_empty() && self.next.len() < 5 {
            let game = self.games.remove(rand::gen_range(0, self.games.len()));
            self.next.push(game);
        }

        let next = self.next.remove(0);
        self.games.push(next.clone());
        next
    }

    fn choose_boss(&self) -> String {
        self.bosses[rand::gen_range(0, self.bosses.len())].clone()
    }
}

// Adapted from macroquad::storage
mod dispenser {
    use std::any::Any;

    static mut STORAGE: Option<Box<dyn Any>> = None;

    pub fn store<T: Any>(data: T) {
        unsafe {
            STORAGE = Some(Box::new(data));
        };
    }

    pub fn take<T: Any>() -> T {
        unsafe { *STORAGE.take().unwrap().downcast::<T>().unwrap() }
    }
}

trait FramesToRun {
    fn to_run(&mut self) -> u32;
    fn to_run_at_rate(&mut self, playback_rate: f32) -> u32;
}

impl FramesToRun for FrameInfo {
    fn to_run(&mut self) -> u32 {
        self.to_run_at_rate(1.0)
    }
    fn to_run_at_rate(&mut self, playback_rate: f32) -> u32 {
        if self.steps_taken == 0 {
            self.first_frame_time = macroquad::time::get_time();
        }
        let time = macroquad::time::get_time();
        let total_time_elapsed = (time - self.first_frame_time) * 1000.0;
        let frame_time = 1000.0 / 60.0;
        let attempts = (total_time_elapsed / frame_time) as i64 - self.steps_taken as i64 + 1;
        let mut frames_to_run = 0;
        for _ in 0..attempts {
            self.steps_taken += 1;
            let mut num_frames = playback_rate.floor();
            let remainder = playback_rate - num_frames;
            if remainder != 0.0 {
                let how_often_extra = 1.0 / remainder;
                if (self.steps_taken as f32 % how_often_extra).floor() == 0.0 {
                    num_frames += 1.0;
                }
            }
            frames_to_run += num_frames as u32;
        }
        match self.remaining() {
            FrameCount::Frames(remaining) => frames_to_run.min(remaining),
            FrameCount::Infinite => frames_to_run,
        }
    }
}

fn preloaded_game(
    games: &HashMap<String, GameData>,
    preloaded_assets: &HashMap<String, Assets>,
    directory: &str,
    filename: &str,
) -> (GameData, Assets) {
    let mode_path = |directory: &str, filename| {
        let mut path = directory.to_string();
        if !path.ends_with('/') {
            path.push('/');
        };
        path.push_str(filename);
        path
    };
    let file_path = mode_path(directory, filename);

    let (game, assets) = (
        games.get(file_path.as_str()),
        preloaded_assets.get(file_path.as_str()),
    );

    if let (Some(game), Some(assets)) = (game, assets) {
        (game.clone(), assets.clone())
    } else {
        let file_path = format!("games/system/{}", filename);
        let game = games[file_path.as_str()].clone();
        let assets = &preloaded_assets[file_path.as_str()];
        (game, assets.clone())
    }
}

#[derive(Debug)]
struct PlayedGames {
    played: HashSet<String>,
    all_games: HashSet<String>,
}

struct MainGame<S> {
    state: S,
    intro_font: Font,
    games: HashMap<String, GameData>,
    preloaded_assets: HashMap<String, Assets>,
    high_scores: HashMap<String, (i32, i32, i32)>,
    played_games: PlayedGames,
    rng: MacroRng,
}

struct LoadingScreen {}

impl MainGame<LoadingScreen> {
    async fn load() -> WeeResult<MainGame<Menu>> {
        let game = LoadedGameData::load("games/system/loading-screen.json").await?;

        let assets = game.assets;

        let mut rng = MacroRng {};

        let mut game = Game::from_data(game.data, &mut rng)?;

        let intro_font = macroquad::text::load_ttf_font("fonts/Roboto-Medium.ttf").await?;

        #[cfg(target_arch = "wasm32")]
        let game_filenames: Vec<String> = vec![
            "games/second/bike.json",
            "games/second/break.json",
            "games/second/dawn.json",
            "games/second/disc.json",
            "games/second/explosion.json",
            "games/second/gravity.json",
            "games/second/jump.json",
            "games/second/knock.json",
            "games/second/mine.json",
            "games/second/racer.json",
            "games/second/slide.json",
            "games/second/stealth.json",
            "games/second/swim.json",
            "games/second/tanks.json",
            "games/second/time.json",
            "games/yeah/baby.json",
            "games/yeah/balloon.json",
            "games/yeah/bird.json",
            "games/yeah/boss.json",
            "games/yeah/boxer.json",
            "games/yeah/cannon.json",
            "games/yeah/cat.json",
            "games/yeah/disgrace.json",
            "games/yeah/hiding.json",
            "games/yeah/mask.json",
            "games/yeah/monkey.json",
            "games/yeah/orange.json",
            "games/yeah/parachute.json",
            "games/yeah/piano.json",
            "games/yeah/planes.json",
            "games/yeah/pumpkin.json",
            "games/yeah/puzzle.json",
            "games/yeah/quake.json",
            "games/yeah/rhinos.json",
            "games/yeah/shake.json",
            "games/yeah/shed.json",
            "games/yeah/titanic.json",
            "games/yeah/wasp.json",
            "games/second/prelude.json",
            "games/system/prelude.json",
            "games/system/pause-menu.json",
            "games/second/interlude.json",
            "games/system/interlude.json",
            "games/second/boss.json",
            "games/second/game-over.json",
            "games/system/game-over.json",
            "games/system/choose-mode.json",
            "games/mine/bong.json",
            "games/bops/cloud.json",
            "games/bops/bowl.json",
            "games/bops/berry.json",
            "games/bops/fruit.json",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        #[cfg(not(target_arch = "wasm32"))]
        let game_filenames = {
            let mut v = Vec::new();

            for entry in WalkDir::new("games").into_iter().filter_map(|e| e.ok()) {
                let metadata = entry.metadata()?;
                let right_extension = match entry.path().extension() {
                    Some(ext) => ext == "json",
                    None => false,
                };

                let filename = entry.path().file_name().unwrap();
                if [
                    "loading-screen.json",
                    "high-scores.json",
                    "played-games.json",
                ]
                .contains(&filename.to_str().unwrap())
                {
                    continue;
                }
                if right_extension && metadata.is_file() {
                    let filename = entry.path().to_str().unwrap();
                    v.push(filename.to_string().replace("\\", "/"));
                }
            }
            v
        };

        let games_to_preload: Vec<String> = game_filenames
            .iter()
            .filter(|f| {
                let enders = [
                    "choose-mode.json",
                    "prelude.json",
                    "interlude.json",
                    "game-over.json",
                    "pause-menu.json",
                    //"boss.json",
                ];
                enders.iter().any(|e| f.ends_with(e))
            })
            .cloned()
            .collect();

        let all_games: HashSet<String> = {
            use std::iter::FromIterator;
            let games_filenames: HashSet<&String> = HashSet::from_iter(game_filenames.iter());
            let games_to_preload: HashSet<&String> = HashSet::from_iter(games_to_preload.iter());
            games_filenames
                .difference(&games_to_preload)
                .map(|s| s.to_string())
                .collect()
        };

        #[cfg(target_arch = "wasm32")]
        let played_games = HashSet::new();

        #[cfg(not(target_arch = "wasm32"))]
        let played_games: HashSet<String> = {
            let path = Path::new("played-games.json");
            log::debug!("path: {:?}", path);
            let json = std::fs::read_to_string(&path);
            if let Ok(json) = json {
                json_from_str(&json)?
            } else {
                HashSet::new()
            }
        };

        log::debug!("Declaring coroutine");
        let resources_loading: Coroutine = start_coroutine(async move {
            log::debug!("Starting coroutine");

            async fn preload_games(
                game_filenames: Vec<String>,
                games_to_preload: Vec<String>,
            ) -> WeeResult<(HashMap<String, GameData>, HashMap<String, Assets>)> {
                let games: HashMap<String, GameData> = {
                    let mut loaded_data = HashMap::new();
                    let mut waiting_data = Vec::new();
                    for filename in &game_filenames {
                        waiting_data.push(load_game_data(filename));
                    }

                    // TODO: Use join_all again after finding source of error
                    //let mut data = join_all(waiting_data).await;
                    let mut data = Vec::new();
                    for _ in 0..waiting_data.len() {
                        data.push(waiting_data.remove(0).await);
                    }

                    for filename in &game_filenames {
                        log::debug!("{}", filename);
                        let data = data.remove(0);
                        match data {
                            Ok(data) => {
                                loaded_data.insert(filename.to_string(), data);
                            }
                            Err(error) => {
                                log::error!("Error: Failed to load game {} - {}", filename, error);
                            }
                        }
                    }

                    loaded_data
                };

                let mut preloaded_assets = HashMap::new();
                let mut waiting_data = Vec::new();
                for filename in &games_to_preload {
                    waiting_data.push(LoadedGameData::load(filename));
                }

                let mut data = join_all(waiting_data).await;

                for filename in games_to_preload {
                    let loaded_data = data.remove(0).unwrap();
                    let assets = loaded_data.assets;
                    preloaded_assets.insert(filename, assets);
                }

                log::debug!("Loaded games");

                Ok((games, preloaded_assets))
            }

            dispenser::store(preload_games(game_filenames, games_to_preload).await);
        });

        clear_background(WHITE);

        log::debug!("Started intro");

        let mut drawn_text = HashMap::new();

        assets.music.play(DEFAULT_PLAYBACK_RATE, VOLUME);

        while !resources_loading.is_done() {
            update_frame(&mut game, &assets, DEFAULT_PLAYBACK_RATE, &mut rng)?
                .add_drawn_text(&mut drawn_text);

            draw_game(
                &game,
                &assets.images,
                &assets.fonts,
                &intro_font,
                &drawn_text,
            );

            next_frame().await;
        }

        assets.stop_sounds();

        let (games, preloaded_assets) =
            dispenser::take::<WeeResult<(HashMap<String, GameData>, HashMap<String, Assets>)>>()?;

        Ok(MainGame {
            state: Menu {},
            intro_font,
            games,
            preloaded_assets,
            high_scores: HashMap::new(),
            played_games: PlayedGames {
                played: played_games,
                all_games,
            },
            rng,
        })
    }
}

struct Menu {}

impl MainGame<Menu> {
    async fn run_game_loop(self) -> WeeResult<MainGame<Menu>> {
        let main_game = self
            .pick_games()
            .await?
            .start()
            .await?
            .play_games()
            .await?
            .return_to_menu()
            .await?;
        Ok(main_game)
    }

    async fn pick_games(mut self) -> WeeResult<MainGame<Prelude>> {
        log::debug!("pick_games");
        let filename = "games/system/choose-mode.json";

        let mut game_data = self.games[filename].clone();
        let assets = &self.preloaded_assets["games/system/choose-mode.json"];

        {
            let total = self.played_games.all_games.len();
            let not_played = self
                .played_games
                .all_games
                .difference(&self.played_games.played)
                .count();
            let text_replacements =
                vec![("{GamesCount}", format!("{}/{}", total - not_played, total))];
            for object in game_data.objects.iter_mut() {
                object.replace_text(&text_replacements);

                #[cfg(target_arch = "wasm32")]
                {
                    // TODO: Set switch code is repeated
                    let mut set_switch = |name, pred| {
                        if object.name == name {
                            object.switch = if pred { Switch::On } else { Switch::Off };
                        }
                    };

                    set_switch("Web", true);
                }
            }
        }

        let mut game = Game::from_data(game_data, &mut self.rng)?;

        let mut drawn_text = HashMap::new();

        assets.music.play(DEFAULT_PLAYBACK_RATE, VOLUME);

        let directory;

        'choose_mode_running: loop {
            for _ in 0..game.frames.to_run() {
                update_frame(&mut game, &assets, DEFAULT_PLAYBACK_RATE, &mut self.rng)?
                    .add_drawn_text(&mut drawn_text);

                draw_game(
                    &game,
                    &assets.images,
                    &assets.fonts,
                    &self.intro_font,
                    &drawn_text,
                );

                next_frame().await;

                for (key, object) in game.objects.iter() {
                    if object.switch == SwitchState::SwitchedOn {
                        let pattern = "OpenFolder:";
                        if let Some(game_directory) = key.strip_prefix(pattern) {
                            directory = game_directory.to_string();
                            break 'choose_mode_running;
                        }
                        if key == "Shuffle" {
                            directory = "games".to_string();
                            break 'choose_mode_running;
                        }
                        if key == "Back" {
                            std::process::exit(0)
                        }
                    }
                }
            }
        }

        assets.stop_sounds();

        Ok(MainGame {
            state: Prelude { directory },
            intro_font: self.intro_font,
            games: self.games,
            preloaded_assets: self.preloaded_assets,
            high_scores: self.high_scores,
            played_games: self.played_games,
            rng: self.rng,
        })
    }
}

struct Prelude {
    directory: String,
}

impl MainGame<Prelude> {
    async fn start(mut self) -> WeeResult<MainGame<Interlude>> {
        log::debug!("prelude");

        let (game, assets) = preloaded_game(
            &self.games,
            &self.preloaded_assets,
            &self.state.directory,
            "prelude.json",
        );

        let mut drawn_text = HashMap::new();

        let mut game = Game::from_data(game, &mut self.rng)?;

        assets.music.play(DEFAULT_PLAYBACK_RATE, VOLUME);

        while game.frames.remaining() != FrameCount::Frames(0) {
            for _ in 0..game.frames.to_run() {
                if update_frame(&mut game, &assets, DEFAULT_PLAYBACK_RATE, &mut self.rng)?
                    .add_drawn_text(&mut drawn_text)
                    .should_end_early()
                {
                    break;
                }

                draw_game(
                    &game,
                    &assets.images,
                    &assets.fonts,
                    &self.intro_font,
                    &drawn_text,
                );

                next_frame().await;
            }
        }

        assets.stop_sounds();

        let games_list = GamesList::from_directory(&self.games, self.state.directory);

        Ok(MainGame {
            state: Interlude {
                progress: Progress::new(),
                games_list,
            },
            intro_font: self.intro_font,
            games: self.games,
            preloaded_assets: self.preloaded_assets,
            high_scores: self.high_scores,
            played_games: self.played_games,
            rng: self.rng,
        })
    }
}

struct Interlude {
    progress: Progress,
    games_list: GamesList,
}

impl MainGame<Interlude> {
    async fn play_games(mut self) -> WeeResult<MainGame<GameOver>> {
        loop {
            let next_step = self.load_game().await?;
            self = match next_step {
                NextStep::Play(game) => match game.play().await? {
                    QuittableGame::Continue(game) => game,
                    QuittableGame::Quit(game) => return Ok(game),
                },
                NextStep::Finished(game_over) => {
                    return Ok(game_over);
                }
            }
        }
    }

    async fn load_game(mut self) -> WeeResult<NextStep> {
        log::debug!("interlude");

        let progress = self.state.progress;
        let is_boss_game = progress.score > 0 && (progress.score % BOSS_GAME_INTERVAL == 0);

        let (mut game_data, assets) = preloaded_game(
            &self.games,
            &self.preloaded_assets,
            &self.state.games_list.directory,
            "interlude.json",
        );

        if self.state.progress.lives == 0 {
            {
                let text_replacements = vec![
                    ("{Score}", progress.score.to_string()),
                    ("{Lives}", progress.lives.to_string()),
                    ("{Game}", "game-over.json".to_string()),
                    ("{IntroText}", "Game Over".to_string()),
                ];
                for object in game_data.objects.iter_mut() {
                    let mut set_switch = |name, pred| {
                        if object.name == name {
                            object.switch = if pred { Switch::On } else { Switch::Off };
                        }
                    };
                    set_switch("1", progress.lives >= 1);
                    set_switch("2", progress.lives >= 2);
                    set_switch("3", progress.lives >= 3);
                    set_switch("4", progress.lives >= 4);

                    object.replace_text(&text_replacements);
                }
            }

            let mut game = Game::from_data(game_data, &mut self.rng)?;

            let playback_rate = self.state.progress.playback_rate;

            assets.music.play(playback_rate, VOLUME);

            let mut drawn_text = HashMap::new();

            'final_interlude_loop: while game.frames.remaining() != FrameCount::Frames(0) {
                if self.should_quit(&assets).await? {
                    break 'final_interlude_loop;
                }

                for _ in 0..game.frames.to_run_at_rate(playback_rate) {
                    if update_frame(&mut game, &assets, playback_rate, &mut self.rng)?
                        .add_drawn_text(&mut drawn_text)
                        .should_end_early()
                    {
                        break 'final_interlude_loop;
                    }
                }

                draw_game(
                    &game,
                    &assets.images,
                    &assets.fonts,
                    &self.intro_font,
                    &drawn_text,
                );

                next_frame().await;
            }

            assets.stop_sounds();

            for key in assets.sounds.keys() {
                macroquad::audio::stop_sound(assets.sounds[key]);
            }

            let next_step = NextStep::Finished(MainGame {
                state: GameOver {
                    progress: self.state.progress,
                    directory: self.state.games_list.directory,
                },
                intro_font: self.intro_font,
                games: self.games,
                preloaded_assets: self.preloaded_assets,
                high_scores: self.high_scores,
                played_games: self.played_games,
                rng: self.rng,
            });
            Ok(next_step)
        } else {
            let next_filename = if is_boss_game {
                // TODO: What if no boss games in folder
                self.state.games_list.choose_boss()
            } else {
                self.state.games_list.choose_game()
            };

            let nf = next_filename.clone();

            log::debug!("next filename: {}", next_filename);

            let new_game_data = self.games[&next_filename].clone();

            {
                let text_replacements = vec![
                    ("{Score}", progress.score.to_string()),
                    ("{Lives}", progress.lives.to_string()),
                    (
                        "{Game}",
                        Path::new(&next_filename)
                            .file_stem()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string(),
                    ),
                    (
                        "{IntroText}",
                        new_game_data
                            .intro_text
                            .as_deref()
                            .unwrap_or("")
                            .to_string(),
                    ),
                ];
                for object in game_data.objects.iter_mut() {
                    let mut set_switch = |name, pred| {
                        if object.name == name {
                            object.switch = if pred { Switch::On } else { Switch::Off };
                        }
                    };
                    if let Some(last_game) = progress.last_game {
                        set_switch("Won", last_game.has_won);
                        set_switch("Gained Life", last_game.was_life_gained);
                    }
                    set_switch("1", progress.lives >= 1);
                    set_switch("2", progress.lives >= 2);
                    set_switch("3", progress.lives >= 3);
                    set_switch("4", progress.lives >= 4);
                    set_switch("Boss", is_boss_game);

                    object.replace_text(&text_replacements);
                }
            }

            let mut game = Game::from_data(game_data, &mut self.rng)?;

            let resources_loading = start_coroutine(async move {
                let base_path = Path::new(&nf).parent().unwrap();
                let resources = Assets::load(&new_game_data.asset_files, base_path).await;
                dispenser::store(resources);
            });

            let playback_rate = self.state.progress.playback_rate;

            assets.music.play(playback_rate, VOLUME);

            let mut drawn_text = HashMap::new();

            'interlude_loop: while (game.frames.remaining() != FrameCount::Frames(0))
                || !resources_loading.is_done()
            {
                if self.should_quit(&assets).await? {
                    return Ok(NextStep::Finished(MainGame {
                        state: GameOver {
                            progress: self.state.progress,
                            directory: self.state.games_list.directory,
                        },
                        intro_font: self.intro_font,
                        games: self.games,
                        preloaded_assets: self.preloaded_assets,
                        high_scores: self.high_scores,
                        played_games: self.played_games,
                        rng: self.rng,
                    }));
                }
                // while (game.frames.remaining() != FrameCount::Frames(0) && !game.end_early)

                for _ in 0..game.frames.to_run_at_rate(playback_rate) {
                    if update_frame(&mut game, &assets, playback_rate, &mut self.rng)?
                        .add_drawn_text(&mut drawn_text)
                        .should_end_early()
                        && resources_loading.is_done()
                    {
                        break 'interlude_loop;
                    }
                }

                draw_game(
                    &game,
                    &assets.images,
                    &assets.fonts,
                    &self.intro_font,
                    &drawn_text,
                );

                next_frame().await;
            }

            assets.stop_sounds();

            let assets = dispenser::take::<WeeResult<Assets>>()?;

            self.played_games.played.insert(next_filename.clone());

            let next_step = NextStep::Play(MainGame {
                state: Play {
                    game_data: self.games[&next_filename].clone(),
                    assets,
                    progress: self.state.progress,
                    games_list: self.state.games_list,
                    is_boss_game,
                },
                intro_font: self.intro_font,
                games: self.games,
                preloaded_assets: self.preloaded_assets,
                high_scores: self.high_scores,
                played_games: self.played_games,
                rng: self.rng,
            });
            Ok(next_step)
        }
    }
}

enum NextStep {
    Play(MainGame<Play>),
    Finished(MainGame<GameOver>),
}

async fn run_pause_menu(
    games: &HashMap<String, GameData>,
    preloaded_assets: &HashMap<String, Assets>,
    rng: &mut MacroRng,
    intro_font: &Font,
    assets: &Assets,
) -> WeeResult<bool> {
    #[cfg(target_arch = "wasm32")]
    let is_pause_pressed = || macroquad::input::is_key_pressed(macroquad::input::KeyCode::P);
    #[cfg(not(target_arch = "wasm32"))]
    let is_pause_pressed = || {
        macroquad::input::is_key_pressed(macroquad::input::KeyCode::P)
            || macroquad::input::is_key_pressed(macroquad::input::KeyCode::Escape)
    };
    if is_pause_pressed() {
        let paused_music = assets.music.pause();
        for sound in assets.sounds.values() {
            audio::stop_sound(*sound);
        }

        let (game, pause_menu_assets) =
            preloaded_game(games, preloaded_assets, "games/system/", "pause-menu.json");

        let mut game = Game::from_data(game, rng)?;

        pause_menu_assets.music.play(DEFAULT_PLAYBACK_RATE, VOLUME);

        fn is_switched_on(objects: &Objects, name: &str) -> bool {
            if let Some(obj) = objects.get(name) {
                obj.switch == SwitchState::SwitchedOn
            } else {
                false
            }
        }

        next_frame().await;

        let mut drawn_text = HashMap::new();

        while !is_pause_pressed() && !is_switched_on(&game.objects, "Continue") {
            for _ in 0..game.frames.to_run() {
                update_frame(&mut game, &pause_menu_assets, DEFAULT_PLAYBACK_RATE, rng)?
                    .add_drawn_text(&mut drawn_text);

                draw_game(
                    &game,
                    &pause_menu_assets.images,
                    &pause_menu_assets.fonts,
                    intro_font,
                    &drawn_text,
                );

                if is_switched_on(&game.objects, "Quit") {
                    pause_menu_assets.stop_sounds();
                    if paused_music {
                        assets.music.resume();
                    }
                    return Ok(true);
                }

                next_frame().await;
            }
        }

        pause_menu_assets.stop_sounds();
        if paused_music {
            assets.music.resume();
        }
    }

    Ok(false)
}

impl<T> MainGame<T> {
    async fn should_quit(&mut self, assets: &Assets) -> WeeResult<bool> {
        run_pause_menu(
            &self.games,
            &self.preloaded_assets,
            &mut self.rng,
            &self.intro_font,
            assets,
        )
        .await
    }
}

impl MainGame<Play> {
    async fn should_quit_play(&mut self) -> WeeResult<bool> {
        run_pause_menu(
            &self.games,
            &self.preloaded_assets,
            &mut self.rng,
            &self.intro_font,
            &self.state.assets,
        )
        .await
    }
}

struct Play {
    game_data: GameData,
    assets: Assets,
    progress: Progress,
    games_list: GamesList,
    is_boss_game: bool,
}

impl MainGame<Play> {
    async fn play(mut self) -> WeeResult<QuittableGame<Interlude>> {
        log::debug!("play");
        log::debug!("playback rate: {}", self.state.progress.playback_rate);

        // TODO: Remove clone
        let mut game = Game::from_data(self.state.game_data.clone(), &mut self.rng)?;
        game.difficulty = self.state.progress.difficulty;

        let playback_rate = if self.state.is_boss_game {
            self.state.progress.boss_playback_rate
        } else {
            self.state.progress.playback_rate
        };

        let mut drawn_text = HashMap::new();
        self.state.assets.music.play(playback_rate, VOLUME);

        'play_loop: while game.frames.remaining() != FrameCount::Frames(0) {
            if self.should_quit_play().await? {
                return Ok(QuittableGame::Quit(MainGame {
                    state: GameOver {
                        progress: self.state.progress,
                        directory: self.state.games_list.directory,
                    },
                    intro_font: self.intro_font,
                    games: self.games,
                    preloaded_assets: self.preloaded_assets,
                    high_scores: self.high_scores,
                    played_games: self.played_games,
                    rng: self.rng,
                }));
            }

            for _ in 0..game.frames.to_run_at_rate(playback_rate) {
                if update_frame(&mut game, &self.state.assets, playback_rate, &mut self.rng)?
                    .add_drawn_text(&mut drawn_text)
                    .should_end_early()
                {
                    break 'play_loop;
                }
            }

            draw_game(
                &game,
                &self.state.assets.images,
                &self.state.assets.fonts,
                &self.intro_font,
                &drawn_text,
            );

            next_frame().await;
        }

        self.state.assets.stop_sounds();

        let has_won = matches!(
            game.status.next_frame,
            WinStatus::Won | WinStatus::HasBeenWon
        );
        self.state.progress.update(has_won, self.state.is_boss_game);

        Ok(QuittableGame::Continue(MainGame {
            state: Interlude {
                progress: self.state.progress,
                games_list: self.state.games_list,
            },
            intro_font: self.intro_font,
            games: self.games,
            preloaded_assets: self.preloaded_assets,
            high_scores: self.high_scores,
            played_games: self.played_games,
            rng: self.rng,
        }))
    }
}

enum QuittableGame<T> {
    Continue(MainGame<T>),
    Quit(MainGame<GameOver>),
}

struct GameOver {
    progress: Progress,
    directory: String,
}

impl MainGame<GameOver> {
    async fn return_to_menu(mut self) -> WeeResult<MainGame<Menu>> {
        log::debug!("return to menu");

        let (mut game_data, assets) = preloaded_game(
            &self.games,
            &self.preloaded_assets,
            &self.state.directory,
            "game-over.json",
        );

        #[cfg(not(target_arch = "wasm32"))]
        let mut high_scores: (i32, i32, i32) = {
            let path = Path::new(&self.state.directory).join("high-scores.json");
            log::debug!("path: {:?}", path);
            let json = std::fs::read_to_string(&path);
            if let Ok(json) = json {
                json_from_str(&json)?
            } else {
                (0, 0, 0)
            }
        };

        #[cfg(target_arch = "wasm32")]
        let mut high_scores = self
            .high_scores
            .entry(self.state.directory)
            .or_insert((0, 0, 0));

        let progress = self.state.progress;
        let score = progress.score;

        let high_score_position: Option<i32> = if score >= high_scores.0 {
            high_scores.2 = high_scores.1;
            high_scores.1 = high_scores.0;
            high_scores.0 = score;
            Some(1)
        } else if score >= high_scores.1 {
            high_scores.2 = high_scores.1;
            high_scores.1 = score;
            Some(2)
        } else if score >= high_scores.2 {
            high_scores.2 = score;
            Some(3)
        } else {
            None
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = Path::new(&self.state.directory).join("high-scores.json");
            let s = serde_json::to_string(&high_scores)?;
            std::fs::write(&path, s).unwrap_or_else(|e| log::error!("{}", e));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = Path::new("played-games.json");
            let s = serde_json::to_string(&self.played_games.played)?;
            std::fs::write(&path, s).unwrap_or_else(|e| log::error!("{}", e));
        }

        let text_replacements = vec![
            ("{Score}", progress.score.to_string()),
            ("{Lives}", progress.lives.to_string()),
            ("{1st}", high_scores.0.to_string()),
            ("{2nd}", high_scores.1.to_string()),
            ("{3rd}", high_scores.2.to_string()),
        ];
        for object in game_data.objects.iter_mut() {
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

        let mut game = Game::from_data(game_data, &mut self.rng)?;

        let mut drawn_text = HashMap::new();

        assets.music.play(1.0, VOLUME);

        while game.frames.remaining() != FrameCount::Frames(0) {
            for _ in 0..game.frames.to_run() {
                if update_frame(&mut game, &assets, DEFAULT_PLAYBACK_RATE, &mut self.rng)?
                    .add_drawn_text(&mut drawn_text)
                    .should_end_early()
                {
                    break;
                }

                draw_game(
                    &game,
                    &assets.images,
                    &assets.fonts,
                    &self.intro_font,
                    &drawn_text,
                );

                next_frame().await;
            }
        }

        assets.stop_sounds();

        Ok(MainGame {
            state: Menu {},
            intro_font: self.intro_font,
            games: self.games,
            preloaded_assets: self.preloaded_assets,
            high_scores: self.high_scores,
            played_games: self.played_games,
            rng: self.rng,
        })
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Weegames".to_string(),
        window_width: 800,
        window_height: 450,
        fullscreen: false,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    log::debug!("Start game");

    macroquad::rand::srand(macroquad::miniquad::date::now() as _);

    let main_game = MainGame::<LoadingScreen>::load().await;

    let mut main_game = match main_game {
        Ok(main_game) => main_game,
        Err(error) => {
            let error = format!("Failed to load game: {}", error);
            for _ in 0..300 {
                clear_background(BLACK);
                macroquad::text::draw_text(&error, 0.0, 64.0, 64.0, WHITE);
                next_frame().await;
            }
            panic!("Failed to load game");
        }
    };

    loop {
        let result = main_game.run_game_loop().await;
        main_game = match result {
            Ok(main_game) => main_game,
            Err(error) => {
                let error = format!("Failed to load game: {}", error);
                for _ in 0..300 {
                    clear_background(BLACK);
                    macroquad::text::draw_text(&error, 0.0, 64.0, 64.0, WHITE);
                    next_frame().await;
                }
                MainGame::<LoadingScreen>::load().await.unwrap()
            }
        }
    }
}
