use sdlglue::{Model, Renderer, Texture};
use wee_common::{
    Colour, Flip, Rect, Size, Vec2, Warn, WeeResult, AABB, PROJECTION_HEIGHT, PROJECTION_WIDTH,
};

use cute_c2::{self as c2, prelude::*};
use indexmap::IndexMap;
use rand::{seq::SliceRandom, thread_rng, Rng};
use sdl2::{
    keyboard::Scancode,
    ttf::{Font, Sdl2TtfContext as TtfContext},
    EventPump,
};
use serde::{Deserialize, Serialize};
use sfml::{
    audio::{Music as SfmlMusic, Sound, SoundBuffer, SoundSource, SoundStatus},
    system::{SfBox, Time as SfmlTime},
};
use std::{
    collections::{HashMap, HashSet},
    default::Default,
    fmt, fs,
    ops::Not,
    path::Path,
    process, str, thread,
    time::{Duration, Instant},
};

const FPS: f32 = 60.0;
pub const DEFAULT_GAME_SPEED: f32 = 1.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialiseObject {
    pub name: String,
    pub sprite: Sprite,
    pub position: Vec2,
    pub size: Size,
    pub angle: f32,
    pub origin: Option<Vec2>,
    pub collision_area: Option<AABB>,
    pub flip: Flip,
    pub layer: u8,
    pub switch: Switch,
    pub instructions: Vec<Instruction>,
}

impl Default for SerialiseObject {
    fn default() -> SerialiseObject {
        SerialiseObject {
            name: "test".to_string(),
            sprite: Sprite::Colour(Colour::white()),
            position: Vec2::zero(),
            size: Size::new(100.0, 100.0),
            angle: 0.0,
            origin: None,
            collision_area: None,
            flip: Flip::default(),
            layer: 0,
            switch: Switch::Off,
            instructions: Vec::new(),
        }
    }
}

impl SerialiseObject {
    pub fn replace_text(&mut self, score: i32, lives: i32) {
        for instruction in self.instructions.iter_mut() {
            fn replace_text_in_action(action: &mut Action, score: i32, lives: i32) {
                if let Action::DrawText { text, .. } = action {
                    *text = text.replace("{Score}", &score.to_string());
                    *text = text.replace("{Lives}", &lives.to_string());
                } else if let Action::Random { random_actions } = action {
                    for action in random_actions {
                        replace_text_in_action(action, score, lives);
                    }
                }
            }
            for action in instruction.actions.iter_mut() {
                replace_text_in_action(action, score, lives);
            }
        }
    }

    pub fn to_object(self) -> Object {
        let switch = match self.switch {
            Switch::On => SwitchState::On,
            Switch::Off => SwitchState::Off,
        };

        let mut object = Object {
            sprite: self.sprite,
            position: self.position,
            size: self.size,
            angle: self.angle,
            origin: self.origin,
            collision_area: self.collision_area,
            flip: self.flip,
            layer: self.layer,
            switch,
            instructions: self.instructions,
            queued_motion: Vec::new(),
            active_motion: ActiveMotion::Stop,
            animation: AnimationStatus::None,
            timer: None,
        };
        for instruction in object.instructions.iter_mut() {
            for trigger in instruction.triggers.iter_mut() {
                if let Trigger::Time(When::Random { start, end }) = trigger {
                    *trigger = Trigger::Time(When::Exact {
                        time: thread_rng().gen_range(*start, *end + 1),
                    });
                }
            }
        }

        object
    }

    pub fn origin(&self) -> Vec2 {
        self.origin
            .unwrap_or_else(|| Vec2::new(self.half_width(), self.half_height()))
    }

    fn half_width(&self) -> f32 {
        self.size.width / 2.0
    }

    fn half_height(&self) -> f32 {
        self.size.height / 2.0
    }
}

pub trait SerialiseObjectList {
    fn get_obj(&self, name: &str) -> WeeResult<&SerialiseObject>;

    fn sprites(&self) -> HashMap<&str, &Sprite>;
}

impl SerialiseObjectList for Vec<SerialiseObject> {
    fn get_obj(&self, name: &str) -> WeeResult<&SerialiseObject> {
        let index = self.iter().position(|o| o.name == name);
        match index {
            Some(index) => Ok(self.get(index).unwrap()),
            None => Err("Cannot find object".into()), // TODO: Better error message
        }
    }

    fn sprites(&self) -> HashMap<&str, &Sprite> {
        let mut sprites = HashMap::new();
        for obj in self {
            sprites.insert(&*obj.name, &obj.sprite);
        }
        sprites
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerialiseMusic {
    filename: String,
    looped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetFiles {
    pub images: HashMap<String, String>,
    audio: HashMap<String, String>,
    music: Option<SerialiseMusic>,
    fonts: HashMap<String, FontLoadInfo>,
}

impl Default for AssetFiles {
    fn default() -> AssetFiles {
        AssetFiles {
            images: HashMap::new(),
            audio: HashMap::new(),
            music: None,
            fonts: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameData {
    pub format_version: String,
    pub published: bool,
    pub objects: Vec<SerialiseObject>,
    pub background: Vec<BackgroundPart>,
    pub asset_files: AssetFiles,
    pub length: f32,
    pub intro_text: Option<String>,
    pub attribution: String,
}

impl Default for GameData {
    fn default() -> GameData {
        GameData {
            format_version: "0.1".to_string(),
            published: false,
            objects: Vec::new(),
            background: Vec::new(),
            asset_files: AssetFiles::default(),
            length: 4.0,
            intro_text: None,
            attribution: "".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum When {
    Start,
    End,
    Exact { time: u32 },
    Random { start: u32, end: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollisionWith {
    Object { name: String },
    Area(AABB),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseOver {
    Object { name: String },
    Area(AABB),
    Anywhere,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[repr(usize)]
pub enum ButtonState {
    Up = 0,
    Down = 1,
    Press = 2,
    Release = 3,
}

impl ButtonState {
    pub fn update(&mut self, pressed: bool) {
        *self = match *self {
            ButtonState::Up | ButtonState::Release => {
                if pressed {
                    ButtonState::Press
                } else {
                    ButtonState::Up
                }
            }
            ButtonState::Down | ButtonState::Press => {
                if pressed {
                    ButtonState::Down
                } else {
                    ButtonState::Release
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MouseInteraction {
    Button { state: ButtonState },
    Hover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Input {
    Mouse {
        over: MouseOver,
        interaction: MouseInteraction,
    },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum WinStatus {
    Won,
    Lost,
    HasBeenWon,
    HasBeenLost,
    NotYetWon,
    NotYetLost,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum SwitchState {
    On,
    Off,
    SwitchedOn,
    SwitchedOff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyCheck {
    Switch(SwitchState),
    Sprite(Sprite),
    FinishedAnimation,
    Timer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Trigger {
    Time(When),
    Collision(CollisionWith),
    Input(Input),
    WinStatus(WinStatus),
    Random { chance: f32 },
    CheckProperty { name: String, check: PropertyCheck },
}

impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trigger::Time(When::Start) => write!(f, "When the game starts"),
            Trigger::Time(When::End) => write!(f, "When the game ends"),
            Trigger::Time(When::Exact { time }) => {
                write!(f, "When the time is {:.2} seconds", (*time as f32) / 60.0)
            }
            Trigger::Time(When::Random { start, end }) => write!(
                f,
                "At a random time between {:.2} and {:.2} seconds",
                start, end,
            ),
            Trigger::Collision(CollisionWith::Object { name }) => {
                write!(f, "While this object collides with {}", name)
            }
            Trigger::Collision(CollisionWith::Area(area)) => write!(
                f,
                "While this object is inside {}, {} and {}, {}",
                area.min.x, area.min.y, area.max.x, area.max.y
            ),
            Trigger::WinStatus(status) => match status {
                WinStatus::Won => write!(f, "While you have won the game"),
                WinStatus::Lost => write!(f, "While you have lost the game"),
                WinStatus::NotYetWon => write!(f, "While you haven't won"),
                WinStatus::NotYetLost => write!(f, "While you haven't lost"),
                WinStatus::HasBeenWon => write!(f, "When you win the game"),
                WinStatus::HasBeenLost => write!(f, "When you lose the game"),
            },
            Trigger::Input(Input::Mouse { over, interaction }) => {
                if let MouseOver::Anywhere = over {
                    match interaction {
                        MouseInteraction::Button { state } => match state {
                            ButtonState::Press => write!(f, "When the screen is clicked"),
                            ButtonState::Down => write!(f, "While the mouse button is down"),
                            ButtonState::Release => write!(f, "When the mouse button is released"),
                            ButtonState::Up => write!(f, "While the mouse button isn't pressed"),
                        },
                        MouseInteraction::Hover => {
                            write!(f, "While the mouse hovers over the screen (always true)")
                        }
                    }
                } else {
                    let clicked_object_string = |clicked_object: &MouseOver| match clicked_object {
                        MouseOver::Object { name } => name.clone(),
                        MouseOver::Area(area) => format!(
                            "the area between {}, {} and {}, {}",
                            area.min.x, area.min.y, area.max.x, area.max.y
                        ),
                        _ => unreachable!(),
                    };
                    let clicked_object = clicked_object_string(over);
                    match interaction {
                        MouseInteraction::Button { state } => match state {
                            ButtonState::Press => write!(f, "When {} is clicked", clicked_object),
                            ButtonState::Down => write!(
                                f,
                                "While the mouse cursor is over {} and the mouse button is down",
                                clicked_object
                            ),
                            ButtonState::Release => write!(
                                f,
                                "When the mouse cursor is over {} and the mouse button is released",
                                clicked_object
                            ),
                            ButtonState::Up => write!(
                                f,
                                "While the mouse cursor is over {} and the mouse button is up",
                                clicked_object
                            ),
                        },
                        MouseInteraction::Hover => {
                            write!(f, "While the mouse is hovered over {}", clicked_object)
                        }
                    }
                }
            }
            Trigger::CheckProperty { name, check } => match check {
                PropertyCheck::Switch(switch) => match switch {
                    SwitchState::On => write!(f, "While {}'s switch is on", name),
                    SwitchState::Off => write!(f, "While {}'s switch is off", name),
                    SwitchState::SwitchedOn => write!(f, "When {} is switched on", name),
                    SwitchState::SwitchedOff => write!(f, "When {} is switched off", name),
                },
                PropertyCheck::Sprite(sprite) => {
                    write!(f, "While {}'s image is {:?}", name, sprite)
                }
                PropertyCheck::FinishedAnimation => {
                    write!(f, "When {}'s animation is finished", name)
                }
                PropertyCheck::Timer => write!(f, "When this object's timer hits zero"),
            },
            Trigger::Random { chance } => write!(f, "With a {}% chance", chance * 100.0),
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Effect {
    Freeze,
    None,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Angle {
    Current,
    Degrees(f32),
    Random { min: f32, max: f32 },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CompassDirection {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl CompassDirection {
    fn angle(self) -> f32 {
        match self {
            CompassDirection::Up => 0.0,
            CompassDirection::UpRight => 45.0,
            CompassDirection::Right => 90.0,
            CompassDirection::DownRight => 135.0,
            CompassDirection::Down => 180.0,
            CompassDirection::DownLeft => 225.0,
            CompassDirection::Left => 270.0,
            CompassDirection::UpLeft => 315.0,
        }
    }

    fn all_directions() -> Vec<CompassDirection> {
        vec![
            CompassDirection::Up,
            CompassDirection::UpRight,
            CompassDirection::Right,
            CompassDirection::DownRight,
            CompassDirection::Down,
            CompassDirection::DownLeft,
            CompassDirection::Left,
            CompassDirection::UpLeft,
        ]
    }

    fn to_vector(self, speed: Speed) -> Vec2 {
        vector_from_angle(self.angle(), speed)
    }
}

impl fmt::Display for CompassDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompassDirection::Up => write!(f, "up"),
            CompassDirection::UpRight => write!(f, "up-right"),
            CompassDirection::Right => write!(f, "right"),
            CompassDirection::DownRight => write!(f, "down-right"),
            CompassDirection::Down => write!(f, "down"),
            CompassDirection::DownLeft => write!(f, "down-left"),
            CompassDirection::Left => write!(f, "left"),
            CompassDirection::UpLeft => write!(f, "up-left"),
        }
    }
}

fn gen_in_range(min: f32, max: f32) -> f32 {
    if min > max {
        thread_rng().gen_range(max, min)
    } else if max > min {
        thread_rng().gen_range(min, max)
    } else {
        min
    }
}

fn angle_from_direction(direction: &MovementDirection, object: &Object) -> f32 {
    match direction {
        MovementDirection::Angle(angle) => match angle {
            Angle::Current => object.angle,
            Angle::Degrees(degrees) => *degrees,
            Angle::Random { min, max } => gen_in_range(*min, *max),
        },
        MovementDirection::Direction {
            possible_directions,
        } => {
            let possible_directions = if !possible_directions.is_empty() {
                possible_directions.iter().cloned().collect()
            } else {
                CompassDirection::all_directions()
            };
            let dir = possible_directions.choose(&mut thread_rng()).unwrap();
            dir.angle()
        }
    }
}

fn vector_from_angle(angle: f32, speed: Speed) -> Vec2 {
    let speed = speed.as_value();
    let angle = (angle - 90.0).to_radians();
    Vec2::new(speed * angle.cos(), speed * angle.sin())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MovementDirection {
    Angle(Angle),
    Direction {
        possible_directions: HashSet<CompassDirection>,
    },
}

impl MovementDirection {
    fn angle(&self, object: &Object) -> f32 {
        angle_from_direction(self, object)
    }
    fn to_vector(&self, object: &Object, speed: Speed) -> Vec2 {
        vector_from_angle(self.angle(object), speed)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Speed {
    VerySlow,
    Slow,
    Normal,
    Fast,
    VeryFast,
    Value(f32),
}

impl Speed {
    fn as_value(self) -> f32 {
        match self {
            Speed::VerySlow => 4.0,
            Speed::Slow => 8.0,
            Speed::Normal => 12.0,
            Speed::Fast => 16.0,
            Speed::VeryFast => 20.0,
            Speed::Value(value) => value,
        }
    }

    fn to_animation_time(self) -> u32 {
        match self {
            Speed::VerySlow => 32,
            Speed::Slow => 16,
            Speed::Normal => 18,
            Speed::Fast => 4,
            Speed::VeryFast => 2,
            Speed::Value(value) => value as u32,
        }
    }
}

impl fmt::Display for Speed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Speed::VerySlow => write!(f, "very slow"),
            Speed::Slow => write!(f, "slow"),
            Speed::Normal => write!(f, "normal"),
            Speed::Fast => write!(f, "fast"),
            Speed::VeryFast => write!(f, "very fast"),
            Speed::Value(value) => write!(f, "speed: {}", value),
        }
    }
}

fn random_velocity(speed: Speed) -> Vec2 {
    let speed = speed.as_value();
    let random_speed = || thread_rng().gen_range(-speed, speed);
    Vec2::new(random_speed(), random_speed())
}

fn clamp_position(position: &mut Vec2, area: AABB) {
    position.x = position.x.min(area.max.x).max(area.min.x);
    position.y = position.y.min(area.max.y).max(area.min.y);
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelativeTo {
    CurrentPosition,
    CurrentAngle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JumpLocation {
    Point(Vec2),
    Area(AABB),
    Relative { to: RelativeTo, distance: Vec2 },
    Object { name: String },
    Mouse,
    ClampPosition { area: AABB },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BounceDirection {
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MovementType {
    Wiggle,
    Insect,
    Reflect {
        initial_direction: MovementDirection,
        movement_handling: MovementHandling,
    },
    Bounce {
        initial_direction: Option<BounceDirection>,
    },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum MovementHandling {
    Anywhere,
    TryNotToOverlap,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Target {
    Object { name: String },
    Mouse,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum TargetType {
    Follow,
    StopWhenReached,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Motion {
    GoStraight {
        direction: MovementDirection,
        speed: Speed,
    },
    JumpTo(JumpLocation),
    Roam {
        movement_type: MovementType,
        area: AABB,
        speed: Speed,
    },
    Swap {
        name: String,
    },
    Target {
        target: Target,
        target_type: TargetType,
        offset: Vec2,
        speed: Speed,
    },
    Accelerate {
        direction: MovementDirection,
        speed: Speed,
    },
    Stop,
}

impl fmt::Display for Motion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Motion::Stop => write!(f, "Stop this object"),
            Motion::GoStraight { direction, speed } => match direction {
                MovementDirection::Direction {
                    possible_directions: dirs,
                } => {
                    if dirs.is_empty() {
                        write!(f, "Go {} in a random direction", speed)
                    } else if dirs.len() == 1 {
                        write!(f, "Go {} {}", dirs.iter().next().unwrap(), speed)
                    } else {
                        let dirs: Vec<String> = dirs.iter().map(|dir| dir.to_string()).collect();
                        let dirs = dirs.join(", ");
                        write!(f, "Go {} in a direction chosen from {}", speed, dirs)
                    }
                }
                MovementDirection::Angle(angle) => match angle {
                    Angle::Current => write!(f, "Go {} in this object's current angle", speed),

                    Angle::Degrees(angle) => write!(f, "Go {} at a {} angle", speed, angle),
                    Angle::Random { min, max } => write!(
                        f,
                        "Go {} at a random angle between {} and {}",
                        speed, min, max
                    ),
                },
            },
            Motion::JumpTo(jump_location) => match jump_location {
                JumpLocation::Point(point) => write!(f, "Jump to {}, {}", point.x, point.y),
                JumpLocation::Relative { to, distance } => match to {
                    RelativeTo::CurrentPosition => write!(
                        f,
                        "Jump {}, {} relative to this object's position",
                        distance.x, distance.y
                    ),
                    RelativeTo::CurrentAngle => write!(
                        f,
                        "Jump {}, {} relative to this object's angle",
                        distance.x, distance.y
                    ),
                },
                JumpLocation::Area(area) => write!(
                    f,
                    "Jump to a random location between {}, {} and {}, {}",
                    area.min.x, area.min.y, area.max.x, area.max.y
                ),
                JumpLocation::ClampPosition { area } => write!(
                    f,
                    "Clamp this object's position within {}, {} and {}, {}",
                    area.min.x, area.min.y, area.max.x, area.max.y
                ),
                JumpLocation::Object { name } => write!(f, "Jump to {}'s position", name),
                JumpLocation::Mouse => write!(f, "Jump to the mouse"),
            },
            Motion::Roam {
                movement_type,
                area,
                speed,
            } => match movement_type {
                MovementType::Wiggle => write!(
                    f,
                    "Wiggle {} between {}, {} and {}, {}",
                    speed, area.min.x, area.min.y, area.max.x, area.max.y,
                ),
                MovementType::Insect => write!(
                    f,
                    "Move like an insect {} between {}, {} and {}, {}",
                    speed, area.min.x, area.min.y, area.max.x, area.max.y,
                ),
                MovementType::Reflect {
                    movement_handling, ..
                } => {
                    let movement_handling = match movement_handling {
                        MovementHandling::Anywhere => "",
                        MovementHandling::TryNotToOverlap => {
                            " trying not to overlap with over objects"
                        }
                    };
                    write!(
                        f,
                        "Reflect {} between {}, {} and {}, {}{}",
                        speed, area.min.x, area.min.y, area.max.x, area.max.y, movement_handling,
                    )
                }
                MovementType::Bounce { .. } => write!(
                    f,
                    "Move like an insect {} between {}, {} and {}, {}",
                    speed, area.min.x, area.min.y, area.max.x, area.max.y,
                ),
            },
            Motion::Swap { name } => write!(f, "Swap position with {}", name),
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
                    TargetType::Follow => write!(f, "Follow {} {}{}", name, speed, offset),
                    TargetType::StopWhenReached => write!(f, "Target {} {}{}", name, speed, offset),
                }
            }
            Motion::Accelerate { direction, speed } => match direction {
                MovementDirection::Direction {
                    possible_directions: dirs,
                } => {
                    if dirs.is_empty() {
                        write!(f, "Accelerate {} in a random direction", speed)
                    } else if dirs.len() == 1 {
                        write!(f, "Accelerate {} {}", dirs.iter().next().unwrap(), speed)
                    } else {
                        let dirs: Vec<String> = dirs.iter().map(|dir| dir.to_string()).collect();
                        let dirs = dirs.join(", ");
                        write!(
                            f,
                            "Accelerate {} in a direction chosen from {}",
                            speed, dirs
                        )
                    }
                }
                MovementDirection::Angle(angle) => match angle {
                    Angle::Current => {
                        write!(f, "Accelerate {} in this object's current angle", speed)
                    }

                    Angle::Degrees(angle) => write!(f, "Accelerate {} at a {} angle", speed, angle),
                    Angle::Random { min, max } => write!(
                        f,
                        "Accelerate {} at a random angle between {} and {}",
                        speed, min, max
                    ),
                },
            },
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnimationType {
    Loop,
    PlayOnce,
}

impl Not for AnimationType {
    type Output = AnimationType;

    fn not(self) -> Self::Output {
        match self {
            AnimationType::Loop => AnimationType::PlayOnce,
            AnimationType::PlayOnce => AnimationType::Loop,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AngleSetter {
    Value(f32),
    Increase(f32),
    Decrease(f32),
    Match { name: String },
    Clamp { min: f32, max: f32 },
    RotateToMouse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SizeDifference {
    Value(Size),
    Percent(Size),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SizeSetter {
    Value(Size),
    Grow(SizeDifference),
    Shrink(SizeDifference),
    Clamp { min: Size, max: Size },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Switch {
    On,
    Off,
}

impl Not for Switch {
    type Output = Switch;

    fn not(self) -> Self::Output {
        match self {
            Switch::On => Switch::Off,
            Switch::Off => Switch::On,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlipSetter {
    Flip,
    SetFlip(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerSetter {
    Value(u8),
    Increase,
    Decrease,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertySetter {
    Sprite(Sprite),
    Angle(AngleSetter),
    Size(SizeSetter),
    Switch(Switch),
    Timer { time: u32 },
    FlipHorizontal(FlipSetter),
    FlipVertical(FlipSetter),
    Layer(LayerSetter),
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum TextResize {
    MatchText,
    MatchObject,
}

impl Not for TextResize {
    type Output = TextResize;

    fn not(self) -> Self::Output {
        match self {
            TextResize::MatchText => TextResize::MatchObject,
            TextResize::MatchObject => TextResize::MatchText,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    Win,
    Lose,
    Effect(Effect),
    Motion(Motion),
    PlaySound {
        name: String,
    },
    StopMusic,
    SetProperty(PropertySetter),
    Animate {
        animation_type: AnimationType,
        sprites: Vec<Sprite>,
        speed: Speed,
    },
    DrawText {
        text: String,
        font: String,
        colour: Colour,
        resize: TextResize,
    },
    Random {
        random_actions: Vec<Action>,
    },
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Motion(motion) => write!(f, "{}", motion),
            Action::Win => write!(f, "Win the game"),
            Action::Lose => write!(f, "Lose the game"),
            Action::Effect(effect) => match effect {
                Effect::Freeze => write!(f, "Freeze the screen"),
                Effect::None => write!(f, "Remove screen effects"),
            },
            Action::PlaySound { name } => write!(f, "Play the {} sound", name),
            Action::StopMusic => write!(f, "Stop the music"),
            Action::Animate {
                animation_type,
                speed,
                ..
            } => {
                if let AnimationType::Loop = animation_type {
                    write!(f, "Loops an animation {}", speed)
                } else {
                    write!(f, "Plays an animation once {}", speed)
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
                write!(
                    f,
                    "Draws `{}` using the {} font with colour ({}) {}",
                    text, font, colour, change_size
                )
            }
            Action::SetProperty(PropertySetter::Angle(angle_setter)) => match angle_setter {
                AngleSetter::Value(value) => write!(f, "Set this object's angle to {}", value),
                AngleSetter::Increase(value) => {
                    write!(f, "Increase this object's angle by {}", value)
                }
                AngleSetter::Decrease(value) => {
                    write!(f, "Decrease this object's angle by {}", value)
                }
                AngleSetter::Match { name } => write!(f, "Have the same angle as {}", name),
                AngleSetter::Clamp { min, max } => write!(
                    f,
                    "Clamp this object's angle to between {} and {} degrees",
                    min, max
                ),
                AngleSetter::RotateToMouse => write!(f, "Rotate towards the mouse"),
            },
            Action::SetProperty(PropertySetter::Sprite(sprite)) => {
                write!(f, "Set this object's sprite to {:?}", sprite)
            }
            Action::SetProperty(PropertySetter::Size(size_setter)) => match size_setter {
                SizeSetter::Value(size) => write!(
                    f,
                    "Set this object's width to {} and its height to {}",
                    size.width, size.height
                ),
                SizeSetter::Grow(SizeDifference::Value(size)) => write!(
                    f,
                    "Grow this object's width by {}px and its height by {}px",
                    size.width, size.height
                ),
                SizeSetter::Shrink(SizeDifference::Value(size)) => write!(
                    f,
                    "Shrink this object's width by {}px and its height by {}px",
                    size.width, size.height
                ),
                SizeSetter::Grow(SizeDifference::Percent(percent)) => write!(
                    f,
                    "Grow this object's width by {}% and its height by {}%",
                    percent.width * 100.0,
                    percent.height * 100.0
                ),
                SizeSetter::Shrink(SizeDifference::Percent(percent)) => write!(
                    f,
                    "Shrink this object's width by {}% and its height by {}%",
                    percent.width * 100.0,
                    percent.height * 100.0
                ),
                SizeSetter::Clamp { min, max } => write!(
                    f,
                    "Clamp this object's size between {}x{} pixels and {}x{} pixels",
                    min.width, min.height, max.width, max.height
                ),
            },
            Action::SetProperty(PropertySetter::Switch(switch)) => write!(
                f,
                "Set the switch {}",
                if let Switch::On = switch { "on" } else { "off" }
            ),
            Action::SetProperty(PropertySetter::Timer { time }) => {
                write!(f, "Set the timer to {:.2} seconds ", time)
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::Flip)) => {
                write!(f, "Flip this object horizontally")
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::Flip)) => {
                write!(f, "Flip this object vertically")
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::SetFlip(flipped))) => {
                write!(f, "Set this object's horizontal flip to {}", flipped)
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::SetFlip(flipped))) => {
                write!(f, "Set this object's vertical flip to {}", flipped)
            }
            Action::SetProperty(PropertySetter::Layer(layer_setter)) => match layer_setter {
                LayerSetter::Value(value) => write!(f, "Set this object's layer to {}", value),
                LayerSetter::Increase => write!(f, "Increase this object's layer by 1"),
                LayerSetter::Decrease => write!(f, "Decrease this object's layer by 1"),
            },
            Action::Random { .. } => write!(f, "Chooses a random action"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instruction {
    pub triggers: Vec<Trigger>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontLoadInfo {
    pub filename: String,
    pub size: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Sprite {
    Image { name: String },
    Colour(Colour),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundPart {
    pub sprite: Sprite,
    pub area: AABB,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Animation {
    should_loop: bool,
    index: usize,
    sprites: Vec<Sprite>,
    speed: Speed,
    time_to_next_change: u32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum AnimationStatus {
    Animating(Animation),
    Finished,
    None,
}

#[derive(Clone, Debug)]
pub enum ActiveMotion {
    GoStraight {
        velocity: Vec2,
    },
    Roam {
        movement_type: ActiveRoam,
        area: AABB,
        speed: Speed,
    },
    Target {
        target: Target,
        target_type: TargetType,
        offset: Vec2,
        speed: Speed,
    },
    Accelerate {
        velocity: Vec2,
        acceleration: Vec2,
    },
    Stop,
}
#[derive(Clone, Debug)]
pub enum ActiveRoam {
    Wiggle,
    Insect {
        velocity: Vec2,
    },
    Reflect {
        velocity: Vec2,
        movement_handling: MovementHandling,
    },
    Bounce {
        velocity: Vec2,
        acceleration: f32,
        direction: BounceDirection,
    },
}

#[derive(Clone, Debug)]
pub struct Object {
    sprite: Sprite,
    position: Vec2,
    size: Size,
    angle: f32,
    pub origin: Option<Vec2>,
    pub collision_area: Option<AABB>,
    flip: Flip,
    layer: u8,
    instructions: Vec<Instruction>,
    queued_motion: Vec<Motion>,
    active_motion: ActiveMotion,
    switch: SwitchState,
    timer: Option<u32>,
    animation: AnimationStatus,
}

impl Object {
    pub fn origin(&self) -> Vec2 {
        self.origin
            .unwrap_or_else(|| Vec2::new(self.half_width(), self.half_height()))
    }

    fn origin_in_world(&self) -> Vec2 {
        self.top_left() + self.origin()
    }

    fn half_width(&self) -> f32 {
        self.size.width / 2.0
    }

    fn half_height(&self) -> f32 {
        self.size.height / 2.0
    }

    fn top_left(&self) -> Vec2 {
        Vec2::new(
            self.position.x - self.half_width(),
            self.position.y - self.half_height(),
        )
    }

    fn trig_angle(&self) -> f32 {
        (self.angle - 90.0).to_radians()
    }

    fn rect(&self) -> Rect {
        Rect::new(
            self.position.x,
            self.position.y,
            self.size.width,
            self.size.height,
        )
    }

    pub fn bottom(&self) -> f32 {
        self.position.y + self.half_height()
    }

    pub fn right(&self) -> f32 {
        self.position.x + self.half_width()
    }

    fn bottom_right(&self) -> Vec2 {
        Vec2::new(
            self.position.x + self.half_width(),
            self.position.y + self.half_height(),
        )
    }

    pub fn collision_aabb(&self) -> AABB {
        match &self.collision_area {
            Some(mut area) => {
                if self.flip.horizontal {
                    let difference_from_left = area.min.x;
                    let difference_from_right = self.size.width - area.max.x;
                    area.min.x = difference_from_right;
                    area.max.x = self.size.width - difference_from_left;
                }
                if self.flip.vertical {
                    let difference_from_top = area.min.y;
                    let difference_from_bottom = self.size.height - area.max.y;
                    area.min.y = difference_from_bottom;
                    area.max.y = self.size.height - difference_from_top;
                }
                area.move_position(self.top_left())
            }
            None => AABB {
                min: self.top_left(),
                max: self.bottom_right(),
            },
        }
    }

    pub fn poly(&self) -> c2::Poly {
        let collision_aabb = self.collision_aabb();
        let origin = self.origin_in_world();
        let aabb = collision_aabb.move_position(-origin);
        let c2v = |x, y| c2::Vec2::new(x, y);
        let mut points = [
            c2v(aabb.min.x, aabb.min.y),
            c2v(aabb.max.x, aabb.min.y),
            c2v(aabb.max.x, aabb.max.y),
            c2v(aabb.min.x, aabb.max.y),
        ];

        let angle = self.angle.to_radians();
        let c = angle.cos();
        let s = angle.sin();
        for point in points.iter_mut() {
            *point = c2::Vec2::new(point.x * c - point.y * s, point.x * s + point.y * c);
            point.x += origin.x;
            point.y += origin.y;
        }
        c2::Poly::from_slice(&points)
    }

    fn update_timer(&mut self) {
        self.timer = match self.timer {
            Some(time) => {
                if time == 0 {
                    None
                } else {
                    Some(time - 1)
                }
            }
            None => None,
        };
    }

    fn update_animation(&mut self) {
        let mut new_sprite = None;
        match &mut self.animation {
            AnimationStatus::Animating(animation) => {
                if animation.time_to_next_change == 0 {
                    if animation.sprites.is_empty() {
                    } else if animation.index == animation.sprites.len() - 1 {
                        if animation.should_loop {
                            animation.index = 0;
                            animation.time_to_next_change = animation.speed.to_animation_time();
                            new_sprite = Some(animation.sprites[0].clone());
                        } else {
                            self.animation = AnimationStatus::Finished;
                        }
                    } else {
                        animation.index += 1;
                        animation.time_to_next_change = animation.speed.to_animation_time();
                        new_sprite = Some(animation.sprites[animation.index].clone());
                    }
                } else {
                    animation.time_to_next_change -= 1;
                }
            }
            AnimationStatus::Finished => {
                self.animation = AnimationStatus::None;
            }
            AnimationStatus::None => {}
        };
        if let Some(sprite) = new_sprite {
            self.sprite = sprite;
        }
    }

    fn update_switch(&mut self, old_switch: SwitchState) {
        if self.switch == SwitchState::SwitchedOn
            && (old_switch == SwitchState::SwitchedOn || old_switch == SwitchState::On)
        {
            self.switch = SwitchState::On;
        } else if self.switch == SwitchState::SwitchedOff
            && (old_switch == SwitchState::SwitchedOff || old_switch == SwitchState::Off)
        {
            self.switch = SwitchState::Off;
        }
    }
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
struct GameStatus {
    current: WinStatus,
    next_frame: WinStatus,
}

#[derive(Copy, Clone)]
struct Mouse {
    position: Vec2,
    state: ButtonState,
}

impl Mouse {
    fn new() -> Mouse {
        Mouse {
            position: Vec2::zero(),
            state: ButtonState::Up,
        }
    }

    fn update(&mut self, event_pump: &mut EventPump, window_size: (u32, u32)) {
        let mouse_state = event_pump.mouse_state();
        let calc_mouse_position =
            |p, window_size, projection| p as f32 / window_size as f32 * projection;
        self.position = Vec2::new(
            calc_mouse_position(mouse_state.x(), window_size.0, PROJECTION_WIDTH),
            calc_mouse_position(mouse_state.y(), window_size.1, PROJECTION_HEIGHT),
        );

        self.state.update(mouse_state.left());
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IntroFontConfig {
    info: FontLoadInfo,
    outline_width: Option<u16>,
}

pub struct IntroFont<'a, 'b> {
    main: Font<'a, 'b>,
    outline: Option<Font<'a, 'b>>,
    pub ttf_context: &'a TtfContext,
}

impl<'a, 'b> IntroFont<'a, 'b> {
    pub fn load(
        intro_font: &IntroFontConfig,
        ttf_context: &'a TtfContext,
    ) -> WeeResult<IntroFont<'a, 'b>> {
        let main = ttf_context.load_font(&intro_font.info.filename, intro_font.info.size as u16)?;
        let outline = if let Some(outline_width) = intro_font.outline_width {
            if outline_width > 0 {
                let mut outline = ttf_context
                    .load_font(&intro_font.info.filename, intro_font.info.size as u16)?;
                outline.set_outline_width(outline_width);
                Some(outline)
            } else {
                None
            }
        } else {
            None
        };
        Ok(IntroFont {
            main,
            outline,
            ttf_context,
        })
    }
}

trait ObjectList {
    fn get_obj(&self, name: &str) -> WeeResult<&Object>;

    fn from_serialised(objects: Vec<SerialiseObject>) -> Self;
}

impl ObjectList for Objects {
    fn get_obj(&self, name: &str) -> WeeResult<&Object> {
        self.get(name)
            .ok_or_else(|| format!("Couldn't find object with name {}", name).into())
    }

    fn from_serialised(objects: Vec<SerialiseObject>) -> Objects {
        let mut new_objects = Objects::new();

        for object in objects {
            new_objects.insert(object.name.clone(), object.to_object());
        }

        new_objects
    }
}

pub trait ImageList {
    fn get_image(&self, name: &str) -> WeeResult<&Texture>;
}

impl ImageList for Images {
    fn get_image(&self, name: &str) -> WeeResult<&Texture> {
        self.get(name)
            .ok_or_else(|| format!("Couldn't find image with name {}", name).into())
    }
}

pub type Objects = IndexMap<String, Object>;
pub type Images = HashMap<String, Texture>;
type Sounds = HashMap<String, SfBox<SoundBuffer>>;
type Fonts<'a, 'b> = HashMap<String, Font<'a, 'b>>;

struct Music {
    data: SfmlMusic,
    looped: bool,
}

pub trait LoadImages {
    fn load<P: AsRef<Path>>(
        image_filenames: &HashMap<String, String>,
        base_path: P,
    ) -> WeeResult<Images>;
}
impl LoadImages for Images {
    fn load<P: AsRef<Path>>(
        image_filenames: &HashMap<String, String>,
        base_path: P,
    ) -> WeeResult<Images> {
        let mut images = Images::new();
        let base_path = base_path.as_ref().join("images");

        for (key, path) in image_filenames {
            let path = base_path.join(path);

            let texture = Texture::from_file(path)?;
            images.insert(key.to_string(), texture);
        }

        Ok(images)
    }
}

trait LoadSounds {
    fn load<P: AsRef<Path>>(
        audio_filenames: &HashMap<String, String>,
        base_path: P,
    ) -> WeeResult<Sounds>;
}
impl LoadSounds for Sounds {
    fn load<P: AsRef<Path>>(
        audio_filenames: &HashMap<String, String>,
        base_path: P,
    ) -> WeeResult<Sounds> {
        let mut sounds = HashMap::new();
        let base_path = base_path.as_ref().join("audio");

        for (key, filename) in audio_filenames {
            let path = base_path.join(filename);
            let buffer = SoundBuffer::from_file(path.to_str().unwrap())
                .ok_or(format!("Couldn't load file {}", filename))?;
            sounds.insert(key.to_string(), buffer);
        }
        Ok(sounds)
    }
}

trait LoadMusic {
    fn load<P: AsRef<Path>>(
        music_name: &Option<SerialiseMusic>,
        base_path: P,
    ) -> WeeResult<Option<Music>>;
}

impl LoadMusic for Music {
    fn load<P: AsRef<Path>>(
        music_name: &Option<SerialiseMusic>,
        base_path: P,
    ) -> WeeResult<Option<Music>> {
        let base_path = base_path.as_ref().join("audio");

        match music_name {
            Some(music_info) => {
                let path = base_path.join(&music_info.filename);
                let music = SfmlMusic::from_file(path.to_str().unwrap())
                    .ok_or(format!("Couldn't load {}", music_info.filename))?;
                let music = Music {
                    data: music,
                    looped: music_info.looped,
                };
                Ok(Some(music))
            }
            None => Ok(None),
        }
    }
}

pub struct Assets<'a, 'b> {
    images: Images,
    sounds: Sounds,
    music: Option<Music>,
    fonts: Fonts<'a, 'b>,
}

impl<'a, 'b> Default for Assets<'a, 'b> {
    fn default() -> Assets<'a, 'b> {
        Assets {
            images: HashMap::new(),
            sounds: HashMap::new(),
            music: None,
            fonts: HashMap::new(),
        }
    }
}

impl<'a, 'b> Assets<'a, 'b> {
    pub fn load<P: AsRef<Path>>(
        asset_filenames: &AssetFiles,
        game_path: P,
        ttf_context: &'a TtfContext,
    ) -> WeeResult<Assets<'a, 'b>> {
        let base_path = game_path.as_ref().parent().unwrap();

        let images = Images::load(&asset_filenames.images, base_path)?;
        let sounds = Sounds::load(&asset_filenames.audio, base_path)?;
        let music = Music::load(&asset_filenames.music, base_path)?;
        let fonts = Fonts::load(&asset_filenames.fonts, base_path, ttf_context)?;

        Ok(Assets {
            images,
            sounds,
            music,
            fonts,
        })
    }

    fn start_music(&mut self, playback_rate: f32, volume: f32) {
        if let Some(music) = &mut self.music {
            music.data.set_playing_offset(SfmlTime::seconds(0.0));
            music.data.set_pitch(playback_rate);
            music.data.set_volume(volume);
            music.data.set_looping(music.looped);
            music.data.play();
        }
    }
}

trait LoadFonts<'a, 'b> {
    fn load<P: AsRef<Path>>(
        font_filenames: &HashMap<String, FontLoadInfo>,
        base_path: P,
        ttf_context: &'a TtfContext,
    ) -> WeeResult<Fonts<'a, 'b>>;
}
impl<'a, 'b> LoadFonts<'a, 'b> for Fonts<'a, 'b> {
    fn load<P: AsRef<Path>>(
        font_filenames: &HashMap<String, FontLoadInfo>,
        base_path: P,
        ttf_context: &'a TtfContext,
    ) -> WeeResult<Fonts<'a, 'b>> {
        let mut fonts = Fonts::new();
        let base_path = base_path.as_ref().join("fonts");
        for (key, info) in font_filenames {
            let path = base_path.join(&info.filename);
            let texture = ttf_context.load_font(path, info.size as u16)?;
            fonts.insert(key.to_string(), texture);
        }
        Ok(fonts)
    }
}

#[derive(Copy, Clone, Debug)]
struct FrameInfo {
    total: u32,
    ran: u32,
    steps_taken: u32,
    start_time: Instant,
    to_run: u32,
}

impl FrameInfo {
    fn remaining(self) -> u32 {
        (self.total - self.ran).max(0)
    }

    fn is_final(self) -> bool {
        self.remaining() == 1
    }
}

pub type IntroText = [Option<Texture>; 2];

pub trait DrawIntroText {
    fn new(intro_font: &IntroFont, text: &Option<String>) -> Self;
}

impl DrawIntroText for IntroText {
    fn new(intro_font: &IntroFont, text: &Option<String>) -> IntroText {
        fn draw_intro_text(font: &Font, text: &Option<String>, colour: Colour) -> Option<Texture> {
            text.as_ref()
                .map(|text| Texture::text(font, text, colour).warn().ok().flatten())
                .flatten()
        }

        let intro_text_main = draw_intro_text(&intro_font.main, text, Colour::white());
        let intro_text_outline = if let Some(outline) = &intro_font.outline {
            draw_intro_text(outline, text, Colour::black())
        } else {
            None
        };

        [intro_text_main, intro_text_outline]
    }
}

fn json_from_str<'a, T: Deserialize<'a>>(text: &'a str) -> WeeResult<T> {
    match serde_json::from_str(text) {
        Ok(data) => Ok(data),
        Err(error) => Err(Box::new(error)),
    }
}

#[derive(Debug, Copy, Clone)]
pub struct GameSettings {
    pub volume: f32,
    pub render_each_frame: bool,
}

pub struct LoadedGame<'a, 'b> {
    pub objects: Vec<SerialiseObject>,
    background: Vec<BackgroundPart>,
    assets: Assets<'a, 'b>,
    intro_text: IntroText,
    intro_font: &'a IntroFont<'a, 'b>,
    total_frames: u32,
}

impl<'a, 'b> LoadedGame<'a, 'b> {
    pub fn load<P: AsRef<Path>>(
        path: P,
        intro_font: &'a IntroFont<'a, 'b>,
    ) -> WeeResult<LoadedGame<'a, 'b>> {
        let json_string = fs::read_to_string(&path)?;

        let game_data = json_from_str(&json_string)?;

        Self::load_from_game_data(game_data, path, intro_font)
    }

    pub fn load_from_game_data<P: AsRef<Path>>(
        game_data: GameData,
        path: P,
        intro_font: &'a IntroFont<'a, 'b>,
    ) -> WeeResult<LoadedGame<'a, 'b>> {
        let objects = game_data.objects.clone();

        let assets = Assets::load(&game_data.asset_files, path, intro_font.ttf_context)?;

        let intro_text = IntroText::new(intro_font, &game_data.intro_text);

        let background = game_data.background;

        Ok(LoadedGame {
            objects,
            background,
            assets,
            intro_text,
            intro_font,
            total_frames: (game_data.length * FPS) as u32,
        })
    }

    pub fn start<'c>(self, playback_rate: f32, settings: GameSettings) -> Game<'a, 'b, 'c> {
        let status = GameStatus {
            current: WinStatus::NotYetWon,
            next_frame: WinStatus::NotYetWon,
        };

        let frames = FrameInfo {
            total: self.total_frames,
            ran: 0,
            steps_taken: 0,
            start_time: Instant::now(),
            to_run: 0,
        };

        let mut assets = self.assets;

        assets.start_music(playback_rate, settings.volume);

        let objects = Objects::from_serialised(self.objects);

        Game {
            objects,
            background: self.background,
            assets,
            intro_font: self.intro_font,
            intro_text: self.intro_text,
            frames,
            status,
            effect: Effect::None,
            mouse: Mouse::new(),
            playing_sounds: Vec::new(),
            drawn_over_text: HashMap::new(),
            playback_rate,
            settings,
        }
    }
}

pub fn is_switched_on(objects: &Objects, name: &str) -> bool {
    if let Some(obj) = objects.get(name) {
        obj.switch == SwitchState::SwitchedOn
    } else {
        false
    }
}

pub enum Completion {
    Finished,
    Quit,
}

pub struct Game<'a, 'b, 'c> {
    pub objects: Objects,
    assets: Assets<'a, 'b>,
    background: Vec<BackgroundPart>,
    intro_font: &'a IntroFont<'a, 'b>,
    intro_text: IntroText,
    frames: FrameInfo,
    status: GameStatus,
    effect: Effect,
    mouse: Mouse,
    playing_sounds: Vec<Sound<'c>>,
    drawn_over_text: HashMap<String, Texture>,
    playback_rate: f32,
    settings: GameSettings,
}

impl<'a, 'b, 'c> Game<'a, 'b, 'c> {
    fn init_frame(&mut self) {
        self.frames.start_time = Instant::now();

        self.frames.steps_taken += 1;

        self.frames.to_run = self.frames_to_run();
    }

    fn frames_to_run(&mut self) -> u32 {
        let mut num_frames = self.playback_rate.floor();
        let remainder = self.playback_rate - num_frames;
        if remainder != 0.0 {
            let how_often_extra = 1.0 / remainder;
            if (self.frames.steps_taken as f32 % how_often_extra).floor() == 0.0 {
                num_frames += 1.0;
            }
        }
        (num_frames as u32).min(self.frames.remaining())
    }

    fn is_finished(&self) -> bool {
        self.frames.remaining() == 0
    }

    pub fn play(
        &mut self,
        renderer: &mut Renderer,
        event_pump: &mut EventPump,
    ) -> WeeResult<Completion> {
        let mut escape = ButtonState::Up;
        'game_running: loop {
            if self.is_finished() {
                break 'game_running;
            }

            self.init_frame();

            let esc_down = |event_pump: &EventPump| {
                event_pump
                    .keyboard_state()
                    .is_scancode_pressed(Scancode::Escape)
            };
            for _ in 0..self.frames.to_run {
                escape.update(esc_down(event_pump));
                if escape == ButtonState::Press {
                    if let Some(music) = &mut self.assets.music {
                        music.data.pause();
                    }
                    for sound in &mut self.playing_sounds {
                        sound.pause();
                    }
                    let mut game =
                        LoadedGame::load("games/system/pause-menu.json", self.intro_font)?
                            .start(DEFAULT_GAME_SPEED, self.settings);
                    'pause_running: loop {
                        sdlglue::set_fullscreen(renderer, &event_pump)?;

                        game.update_and_render_frame(renderer, event_pump)?;

                        escape.update(esc_down(event_pump));
                        if escape == ButtonState::Press || is_switched_on(&game.objects, "Continue")
                        {
                            if let Some(music) = &mut self.assets.music {
                                music.data.play();
                            }
                            for sound in &mut self.playing_sounds {
                                sound.play();
                            }
                            break 'pause_running;
                        }
                        if is_switched_on(&game.objects, "Quit") {
                            return Ok(Completion::Quit);
                        }
                    }
                }
                sdlglue::set_fullscreen(renderer, &event_pump)?;
                self.update_frame(event_pump, renderer.window.size())?;
                if self.settings.render_each_frame {
                    self.render_frame(renderer)?;
                }
            }

            if !self.settings.render_each_frame {
                self.render_frame(renderer)?;
            }

            self.sleep();
        }

        if let Some(music) = &mut self.assets.music {
            music.data.stop();
        }

        Ok(Completion::Finished)
    }

    fn sleep(&self) {
        let elapsed = self.frames.start_time.elapsed().as_nanos();
        let sleep_time = (1_000_000_000u32 / FPS as u32) as u128;
        if elapsed < sleep_time {
            let sleep_time = (sleep_time - elapsed) as u32;
            thread::sleep(Duration::new(0, sleep_time));
        }
    }

    fn update_frame(
        &mut self,
        event_pump: &mut EventPump,
        window_size: (u32, u32),
    ) -> WeeResult<()> {
        if sdlglue::has_quit(event_pump) {
            process::exit(0);
        }

        self.mouse.update(event_pump, window_size);

        let played_sounds = if let Effect::Freeze = self.effect {
            Vec::new()
        } else {
            self.update_objects()?
        };

        self.update_status();

        self.frames.ran += 1;

        self.play_sounds(&played_sounds, self.settings.volume);

        Ok(())
    }

    fn render_frame(&self, renderer: &Renderer) -> WeeResult<()> {
        sdlglue::clear_screen(Colour::white());

        renderer.draw_background(&self.background, &self.assets.images)?;
        renderer.draw_objects(&self.objects, &self.assets.images, &self.drawn_over_text)?;

        const INTRO_TEXT_TIME: u32 = 60;
        if self.frames.ran < INTRO_TEXT_TIME {
            for intro in &self.intro_text {
                if let Some(text) = intro {
                    renderer.prepare(text).draw();
                }
            }
        }

        renderer.present();

        Ok(())
    }

    fn play_sounds(&mut self, played_sounds: &[String], volume: f32) {
        unsafe {
            for name in played_sounds {
                let mut sound = Sound::with_buffer(
                    &*(&self.assets.sounds[name] as *const SfBox<SoundBuffer>) as &SoundBuffer,
                );
                sound.set_volume(volume);
                sound.set_pitch(self.playback_rate);
                sound.play();
                self.playing_sounds.push(sound);
            }
        }

        fn remove_stopped_sounds(playing_sounds: &mut Vec<Sound>) {
            playing_sounds.retain(|sound| match sound.status() {
                SoundStatus::Stopped => false,
                _ => true,
            });
        }

        remove_stopped_sounds(&mut self.playing_sounds);
    }

    fn update_status(&mut self) {
        self.status.current = self.status.next_frame;
        self.status.next_frame = match self.status.next_frame {
            WinStatus::HasBeenWon => WinStatus::Won,
            WinStatus::HasBeenLost => WinStatus::Lost,
            _ => self.status.next_frame,
        };
    }

    fn is_triggered(&self, name: &str, trigger: &Trigger) -> WeeResult<bool> {
        let is_point_in_area = |pos: Vec2, area: AABB| {
            pos.x >= area.min.x && pos.y >= area.min.y && pos.x < area.max.x && pos.y < area.max.y
        };
        let is_mouse_in_area = |mouse: Mouse, area| is_point_in_area(mouse.position, area);
        let c2v = |v: Vec2| c2::Vec2::new(v.x, v.y);
        let triggered = match trigger {
            Trigger::Time(When::Start) => self.frames.ran == 0,
            Trigger::Time(When::End) => self.frames.is_final(),
            Trigger::Time(When::Exact { time }) => self.frames.ran == *time,
            Trigger::Time(When::Random { .. }) => false,
            Trigger::Collision(CollisionWith::Object { name: other_name }) => {
                let other_obj = self.objects.get_obj(other_name)?;

                self.objects[name].poly().collides_with(&other_obj.poly())
            }
            Trigger::Collision(CollisionWith::Area(area)) => {
                let area = c2::AABB::new(c2v(area.min), c2v(area.max));

                self.objects[name].poly().collides_with(&area)
            }
            Trigger::WinStatus(win_status) => match win_status {
                WinStatus::Won => match self.status.current {
                    WinStatus::Won | WinStatus::HasBeenWon => true,
                    _ => false,
                },
                WinStatus::Lost => match self.status.current {
                    WinStatus::Lost | WinStatus::HasBeenLost => true,
                    _ => false,
                },
                WinStatus::NotYetLost => match self.status.current {
                    WinStatus::NotYetLost
                    | WinStatus::NotYetWon
                    | WinStatus::HasBeenWon
                    | WinStatus::Won => true,
                    _ => false,
                },
                WinStatus::NotYetWon => match self.status.current {
                    WinStatus::NotYetWon
                    | WinStatus::NotYetLost
                    | WinStatus::HasBeenLost
                    | WinStatus::Lost => true,
                    _ => false,
                },
                _ => self.status.current == *win_status,
            },
            Trigger::Input(Input::Mouse { over, interaction }) => {
                let is_over = match over {
                    MouseOver::Object { name: other_name } => {
                        let other_obj = self.objects.get_obj(other_name)?;
                        other_obj
                            .poly()
                            .gjk(&c2::Circle::new(c2v(self.mouse.position), 1.0))
                            .use_radius(false)
                            .run()
                            .distance
                            == 0.0
                    }
                    MouseOver::Area(area) => is_mouse_in_area(self.mouse, *area),
                    MouseOver::Anywhere => true,
                };
                is_over
                    && match interaction {
                        MouseInteraction::Button { state } => *state == self.mouse.state,
                        MouseInteraction::Hover => true,
                    }
            }
            Trigger::CheckProperty {
                name: object_name,
                check,
            } => {
                let obj = self.objects.get_obj(object_name)?;
                match check {
                    PropertyCheck::Switch(switch_state) => obj.switch == *switch_state,
                    PropertyCheck::Sprite(sprite) => obj.sprite == *sprite,
                    PropertyCheck::FinishedAnimation => match obj.animation {
                        AnimationStatus::Finished => true,
                        _ => false,
                    },
                    PropertyCheck::Timer => match obj.timer {
                        Some(alarm) => alarm == 0,
                        None => false,
                    },
                }
            }
            Trigger::Random { chance } => {
                let roll = thread_rng().gen::<f32>();
                roll < *chance
            }
        };
        Ok(triggered)
    }

    fn check_triggers(&self, name: &str) -> WeeResult<Vec<Action>> {
        let mut actions = Vec::new();
        for instruction in self.objects[name].instructions.iter() {
            let mut triggered = true;
            for trigger in &instruction.triggers {
                triggered = triggered && self.is_triggered(name, trigger)?;
            }
            if triggered {
                actions.extend(instruction.actions.clone());
            }
        }
        Ok(actions)
    }

    fn apply_actions(
        &mut self,
        name: &str,
        actions: &[Action],
        played_sounds: &mut Vec<String>,
    ) -> WeeResult<()> {
        for action in actions {
            self.apply_action(name, action, played_sounds)?;
        }
        Ok(())
    }

    fn apply_action(
        &mut self,
        name: &str,
        action: &Action,
        played_sounds: &mut Vec<String>,
    ) -> WeeResult<()> {
        let try_to_set_status = |status: &mut GameStatus, opposite, next_frame| {
            *status = match status.current {
                WinStatus::NotYetWon | WinStatus::NotYetLost => {
                    if status.next_frame == opposite {
                        *status
                    } else {
                        GameStatus {
                            current: status.current,
                            next_frame,
                        }
                    }
                }
                _ => *status,
            };
        };
        let try_to_win = |status| {
            try_to_set_status(status, WinStatus::HasBeenLost, WinStatus::HasBeenWon);
        };
        let try_to_lose = |status| {
            try_to_set_status(status, WinStatus::HasBeenWon, WinStatus::HasBeenLost);
        };
        match action {
            Action::Motion(motion) => {
                self.objects[name].queued_motion.push(motion.clone());
            }
            Action::Win => {
                try_to_win(&mut self.status);
            }
            Action::Lose => {
                try_to_lose(&mut self.status);
            }
            Action::Effect(new_effect) => {
                self.effect = *new_effect;
            }
            Action::PlaySound { name: sound_name } => {
                played_sounds.push(sound_name.clone());
            }
            Action::StopMusic => {
                if let Some(music) = &mut self.assets.music {
                    music.data.stop();
                }
            }
            Action::Animate {
                animation_type,
                sprites,
                speed,
            } => {
                let should_loop = match animation_type {
                    AnimationType::Loop => true,
                    AnimationType::PlayOnce => false,
                };
                self.objects[name].animation = AnimationStatus::Animating(Animation {
                    should_loop,
                    sprites: sprites.clone(),
                    index: 0,
                    speed: *speed,
                    time_to_next_change: speed.to_animation_time(),
                });

                let new_sprite = sprites.get(0).cloned();
                if let Some(sprite) = new_sprite {
                    self.objects[name].sprite = sprite;
                }
            }
            Action::DrawText {
                text,
                font,
                colour,
                resize,
            } => {
                let texture = Texture::text(&self.assets.fonts[font], &text, *colour)?;
                match texture {
                    Some(texture) => {
                        if let TextResize::MatchText = resize {
                            self.objects[name].size = Size {
                                width: texture.width as f32,
                                height: texture.height as f32,
                            }
                        }
                        self.drawn_over_text.insert(name.to_string(), texture);
                    }
                    None => {
                        self.drawn_over_text.remove(name);
                    }
                }
            }
            Action::SetProperty(PropertySetter::Angle(angle_setter)) => {
                self.objects[name].angle = match angle_setter {
                    AngleSetter::Value(value) => *value,
                    AngleSetter::Increase(value) => self.objects[name].angle + value,
                    AngleSetter::Decrease(value) => self.objects[name].angle - value,
                    AngleSetter::Match { name: other_name } => {
                        self.objects.get_obj(other_name)?.angle
                    }
                    AngleSetter::Clamp { min, max } => {
                        let mut angle = self.objects[name].angle;
                        if angle < 0.0 {
                            angle += 360.0;
                        }

                        fn clamp_degrees(angle: f32, min: f32, max: f32) -> f32 {
                            fn is_between_angles(angle: f32, min: f32, max: f32) -> bool {
                                if min < max {
                                    angle >= min && angle <= max
                                } else {
                                    angle >= min && angle <= (max + 360.0)
                                        || angle >= (min - 360.0) && angle <= max
                                }
                            }
                            fn distance_between_angles(a: f32, b: f32) -> f32 {
                                let dist1 = (a - b).abs();
                                let dist2 = ((a + 360.0) - b).abs();
                                let dist3 = (a - (b + 360.0)).abs();
                                dist1.min(dist2.min(dist3))
                            }

                            if is_between_angles(angle, min, max) {
                                angle
                            } else if distance_between_angles(angle, min)
                                < distance_between_angles(angle, max)
                            {
                                min
                            } else {
                                max
                            }
                        }
                        clamp_degrees(angle, *min, *max)
                    }
                    AngleSetter::RotateToMouse => {
                        let centre = self.objects[name].origin_in_world();
                        (self.mouse.position.y - centre.y)
                            .atan2(self.mouse.position.x - centre.x)
                            .to_degrees()
                            + 90.0
                    }
                };
            }
            Action::SetProperty(PropertySetter::Sprite(sprite)) => {
                self.objects[name].sprite = sprite.clone();
                self.objects[name].animation = AnimationStatus::None;
            }
            Action::SetProperty(PropertySetter::Size(size_setter)) => {
                let old_size = self.objects[name].size;
                self.objects[name].size = match size_setter {
                    SizeSetter::Value(value) => *value,
                    SizeSetter::Grow(SizeDifference::Value(value)) => Size {
                        width: self.objects[name].size.width + value.width,
                        height: self.objects[name].size.height + value.height,
                    },
                    SizeSetter::Shrink(SizeDifference::Value(value)) => Size {
                        width: self.objects[name].size.width - value.width,
                        height: self.objects[name].size.height - value.height,
                    },
                    SizeSetter::Grow(SizeDifference::Percent(percent)) => {
                        let w = self.objects[name].size.width;
                        let h = self.objects[name].size.height;
                        Size {
                            width: w + w * (percent.width / 100.0),
                            height: h + h * (percent.height / 100.0),
                        }
                    }
                    SizeSetter::Shrink(SizeDifference::Percent(percent)) => {
                        let w = self.objects[name].size.width;
                        let h = self.objects[name].size.height;
                        Size {
                            width: w - w * (percent.width / 100.0),
                            height: h - h * (percent.height / 100.0),
                        }
                    }
                    SizeSetter::Clamp { min, max } => Size {
                        width: self.objects[name].size.width.min(max.width).max(min.width),
                        height: self.objects[name]
                            .size
                            .height
                            .min(max.height)
                            .max(min.height),
                    },
                };
                let size = self.objects[name].size;
                let difference =
                    Size::new(size.width / old_size.width, size.height / old_size.height);
                match &mut self.objects[name].collision_area {
                    Some(area) => {
                        *area = AABB {
                            min: Vec2::new(
                                area.min.x * difference.width,
                                area.min.y * difference.height,
                            ),
                            max: Vec2::new(
                                area.max.x * difference.width,
                                area.max.y * difference.width,
                            ),
                        };
                    }
                    None => {}
                }
            }
            Action::SetProperty(PropertySetter::Switch(switch)) => {
                if *switch == Switch::On && self.objects[name].switch != SwitchState::On {
                    self.objects[name].switch = SwitchState::SwitchedOn;
                } else if *switch == Switch::Off && self.objects[name].switch != SwitchState::Off {
                    self.objects[name].switch = SwitchState::SwitchedOff;
                }
            }
            Action::SetProperty(PropertySetter::Timer { time }) => {
                self.objects[name].timer = Some(*time);
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::Flip)) => {
                self.objects[name].flip.horizontal = !self.objects[name].flip.horizontal;
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::Flip)) => {
                self.objects[name].flip.vertical = !self.objects[name].flip.vertical;
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::SetFlip(flipped))) => {
                self.objects[name].flip.horizontal = *flipped;
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::SetFlip(flipped))) => {
                self.objects[name].flip.vertical = *flipped;
            }
            Action::SetProperty(PropertySetter::Layer(layer_setter)) => {
                self.objects[name].layer = match layer_setter {
                    LayerSetter::Value(value) => *value,
                    LayerSetter::Increase => {
                        self.objects[name].layer
                            + if self.objects[name].layer < std::u8::MAX - 1 {
                                1
                            } else {
                                0
                            }
                    }
                    LayerSetter::Decrease => {
                        self.objects[name].layer - if self.objects[name].layer > 0 { 1 } else { 0 }
                    }
                };
            }
            Action::Random { random_actions } => {
                let action = random_actions.choose(&mut thread_rng());
                if let Some(action) = action {
                    self.apply_action(name, &action, played_sounds)?;
                }
            }
        };
        Ok(())
    }

    fn move_object(&mut self, name: &str) -> WeeResult<()> {
        for mut motion in self.objects[name].queued_motion.clone().into_iter() {
            self.objects[name].active_motion = match &mut motion {
                Motion::GoStraight { direction, speed } => {
                    let velocity = direction.to_vector(&self.objects[name], *speed);
                    ActiveMotion::GoStraight { velocity }
                }
                Motion::JumpTo(jump_location) => {
                    match jump_location {
                        JumpLocation::Point(point) => {
                            self.objects[name].position = *point;
                        }
                        JumpLocation::Relative { to, distance } => match to {
                            RelativeTo::CurrentPosition => {
                                self.objects[name].position += *distance;
                            }
                            RelativeTo::CurrentAngle => {
                                let angle = self.objects[name].trig_angle();
                                self.objects[name].position.x +=
                                    -distance.y * angle.cos() - distance.x * angle.sin();
                                self.objects[name].position.y +=
                                    -distance.y * angle.sin() + distance.x * angle.cos();
                            }
                        },
                        JumpLocation::Area(area) => {
                            fn gen_in_area(area: AABB) -> Vec2 {
                                Vec2::new(
                                    gen_in_range(area.min.x, area.max.x),
                                    gen_in_range(area.min.y, area.max.y),
                                )
                            }
                            self.objects[name].position = gen_in_area(*area);
                        }
                        JumpLocation::ClampPosition { area } => {
                            clamp_position(&mut self.objects[name].position, *area);
                        }
                        JumpLocation::Object { name: other_name } => {
                            self.objects[name].position =
                                self.objects.get_obj(&other_name)?.position;
                        }
                        JumpLocation::Mouse => {
                            self.objects[name].position = self.mouse.position;
                        }
                    }
                    ActiveMotion::Stop
                }
                Motion::Roam {
                    movement_type,
                    area,
                    speed,
                } => {
                    let active_roam = match movement_type {
                        MovementType::Wiggle => ActiveRoam::Wiggle,
                        MovementType::Reflect {
                            initial_direction,
                            movement_handling,
                        } => {
                            let width = area.width();
                            let height = area.height();
                            match initial_direction {
                                MovementDirection::Angle(angle) => {
                                    let angle = match angle {
                                        Angle::Current => self.objects[name].angle,
                                        Angle::Degrees(degrees) => *degrees,
                                        Angle::Random { min, max } => gen_in_range(*min, *max),
                                    };
                                    let velocity = vector_from_angle(angle, *speed);
                                    ActiveRoam::Reflect {
                                        velocity,
                                        movement_handling: *movement_handling,
                                    }
                                }
                                MovementDirection::Direction {
                                    possible_directions,
                                } => {
                                    let enough_horizontal_space =
                                        width < self.objects[name].size.width;
                                    let enough_vertical_space =
                                        height < self.objects[name].size.height;

                                    let possible_directions = if !possible_directions.is_empty() {
                                        possible_directions.iter().cloned().collect()
                                    } else if enough_horizontal_space && enough_vertical_space {
                                        Vec::new()
                                    } else if enough_horizontal_space {
                                        vec![CompassDirection::Up, CompassDirection::Down]
                                    } else if enough_vertical_space {
                                        vec![CompassDirection::Left, CompassDirection::Right]
                                    } else {
                                        CompassDirection::all_directions()
                                    };
                                    let dir = possible_directions.choose(&mut thread_rng());
                                    let velocity = match dir {
                                        Some(dir) => dir.to_vector(*speed),
                                        None => Vec2::zero(),
                                    };
                                    ActiveRoam::Reflect {
                                        velocity,
                                        movement_handling: *movement_handling,
                                    }
                                }
                            }
                        }
                        MovementType::Insect => ActiveRoam::Insect {
                            velocity: random_velocity(*speed),
                        },
                        MovementType::Bounce { initial_direction } => {
                            let acceleration = -2.0 * (area.min.y - area.max.y) / (FPS * FPS);
                            let velocity = {
                                let y_velocity =
                                    2.0 * (area.min.y - self.objects[name].position.y) / FPS;
                                Vec2::new(0.0, y_velocity)
                            };
                            let direction = initial_direction.clone().unwrap_or_else(|| {
                                if thread_rng().gen_range(0, 2) == 0 {
                                    BounceDirection::Left
                                } else {
                                    BounceDirection::Right
                                }
                            });

                            ActiveRoam::Bounce {
                                velocity,
                                direction,
                                acceleration,
                            }
                        }
                    };
                    ActiveMotion::Roam {
                        movement_type: active_roam,
                        area: *area,
                        speed: *speed,
                    }
                }
                Motion::Swap { name: other_name } => {
                    self.objects.get_obj(&other_name)?;
                    let temp = self.objects[other_name].position;
                    self.objects[other_name].position = self.objects[name].position;
                    self.objects[name].position = temp;
                    ActiveMotion::Stop
                }
                Motion::Target {
                    target,
                    target_type,
                    offset,
                    speed,
                } => ActiveMotion::Target {
                    target: target.clone(),
                    target_type: *target_type,
                    offset: *offset,
                    speed: *speed,
                },
                Motion::Accelerate { direction, speed } => {
                    let acceleration = direction.to_vector(&self.objects[name], *speed);
                    let velocity = match &self.objects[name].active_motion {
                        ActiveMotion::Accelerate { velocity, .. } => *velocity,
                        ActiveMotion::GoStraight { velocity } => *velocity,
                        ActiveMotion::Roam { movement_type, .. } => match movement_type {
                            ActiveRoam::Insect { velocity } => *velocity,
                            ActiveRoam::Bounce { velocity, .. } => *velocity,
                            ActiveRoam::Reflect { velocity, .. } => *velocity,
                            _ => Vec2::zero(),
                        },
                        ActiveMotion::Target { .. } => Vec2::zero(),
                        ActiveMotion::Stop => Vec2::zero(),
                    };
                    ActiveMotion::Accelerate {
                        velocity,
                        acceleration,
                    }
                }
                Motion::Stop => ActiveMotion::Stop,
            };
        }
        self.objects[name].queued_motion = Vec::new();

        self.objects[name].active_motion = self.update_active_motion(name)?;

        Ok(())
    }

    fn update_active_motion(&mut self, name: &str) -> WeeResult<ActiveMotion> {
        let active_motion = match self.objects[name].active_motion.clone() {
            ActiveMotion::GoStraight { velocity } => {
                self.objects[name].position += velocity;
                ActiveMotion::GoStraight { velocity }
            }
            ActiveMotion::Roam {
                movement_type,
                area,
                speed,
            } => {
                let movement_type = match movement_type {
                    ActiveRoam::Wiggle => {
                        self.objects[name].position += random_velocity(speed);
                        clamp_position(&mut self.objects[name].position, area);

                        ActiveRoam::Wiggle
                    }
                    ActiveRoam::Insect { mut velocity } => {
                        const CHANGE_DIRECTION_PROBABILTY: f32 = 0.1;
                        if thread_rng().gen::<f32>() < CHANGE_DIRECTION_PROBABILTY {
                            velocity = random_velocity(speed);
                        }
                        self.objects[name].position += velocity;

                        clamp_position(&mut self.objects[name].position, area);

                        ActiveRoam::Insect { velocity }
                    }
                    ActiveRoam::Reflect {
                        mut velocity,
                        movement_handling,
                    } => {
                        if let MovementHandling::TryNotToOverlap = movement_handling {
                            fn calculate_closest_manifold<T: BasicShape>(
                                objects: &Objects,
                                name: &str,
                                poly: T,
                            ) -> (Option<c2::Manifold>, Vec2) {
                                let mut longest_depth = 0.0;
                                let mut closest_manifold = None;
                                let mut position = Vec2::zero();
                                for other_name in objects.keys() {
                                    if other_name != name {
                                        let manifold = poly.manifold(&objects[other_name].poly());
                                        if manifold.count > 0 {
                                            let depth = manifold.depths[0];
                                            if depth > longest_depth || closest_manifold.is_none() {
                                                closest_manifold = Some(manifold);
                                                position = objects[other_name].position;
                                                longest_depth = depth;
                                            }
                                        }
                                    }
                                }
                                (closest_manifold, position)
                            };
                            let (original_manifold, other_position) = calculate_closest_manifold(
                                &self.objects,
                                name,
                                self.objects[name].poly(),
                            );
                            let move_away = |manifold: Option<c2::Manifold>| {
                                if let Some(manifold) = manifold {
                                    let normal = manifold.normal();
                                    let depth = manifold.depths[0];
                                    Vec2::new(normal.x * depth, normal.y * depth)
                                } else {
                                    Vec2::zero()
                                }
                            };
                            self.objects[name].position -= move_away(original_manifold);

                            let closest_manifold = {
                                let moved_poly = {
                                    let transformation = c2::Transformation::new(
                                        c2::Vec2::new(velocity.x, velocity.y),
                                        0.0,
                                    );
                                    (self.objects[name].poly(), transformation)
                                };

                                let (new_manifold, _) =
                                    calculate_closest_manifold(&self.objects, name, moved_poly);

                                let is_moving_towards = {
                                    let to_point = other_position - self.objects[name].position;
                                    (to_point.x * velocity.x + to_point.y * velocity.y) > 0.0
                                };
                                if new_manifold.is_none() && is_moving_towards {
                                    original_manifold
                                } else {
                                    new_manifold
                                }
                            };
                            self.objects[name].position -= move_away(closest_manifold);
                            if let Some(manifold) = closest_manifold {
                                let normal = manifold.normal();
                                let len = (normal.x.powf(2.0) + normal.y.powf(2.0)).sqrt();
                                if len != 0.0 {
                                    let normal = Vec2::new(normal.x / len, normal.y / len);
                                    velocity = velocity - 2.0 * (velocity * normal) * normal;
                                }
                            }
                        }
                        if self.objects[name].position.x + velocity.x < area.min.x {
                            velocity.x = velocity.x.abs();
                        }
                        if self.objects[name].position.x + velocity.x > area.max.x {
                            velocity.x = -velocity.x.abs();
                        }
                        if self.objects[name].position.y + velocity.y < area.min.y {
                            velocity.y = velocity.y.abs();
                        }
                        if self.objects[name].position.y + velocity.y > area.max.y {
                            velocity.y = -velocity.y.abs();
                        }
                        if area.width() < self.objects[name].size.width {
                            velocity.x = 0.0;
                        }
                        if area.height() < self.objects[name].size.height {
                            velocity.y = 0.0;
                        }
                        self.objects[name].position += velocity;

                        ActiveRoam::Reflect {
                            velocity,
                            movement_handling,
                        }
                    }
                    ActiveRoam::Bounce {
                        mut velocity,
                        mut direction,
                        acceleration,
                    } => {
                        let (x, y) = (self.objects[name].position.x, self.objects[name].position.y);
                        if y < area.min.y && velocity.y < 0.0 {
                            velocity.y = 0.0;
                        }
                        if y < area.max.y {
                            velocity.y += acceleration;
                        } else if y > area.max.y {
                            velocity.y = -acceleration * FPS;
                        }
                        if x > area.max.x {
                            direction = BounceDirection::Left;
                        } else if x < area.min.x {
                            direction = BounceDirection::Right;
                        }
                        match direction {
                            BounceDirection::Left => velocity.x = -speed.as_value(),
                            BounceDirection::Right => velocity.x = speed.as_value(),
                        }
                        self.objects[name].position += velocity;

                        ActiveRoam::Bounce {
                            velocity,
                            direction,
                            acceleration,
                        }
                    }
                };
                ActiveMotion::Roam {
                    movement_type,
                    area,
                    speed,
                }
            }
            ActiveMotion::Target {
                target,
                target_type,
                offset,
                speed,
            } => {
                self.objects[name].position = {
                    let other = match &target {
                        Target::Object { name: other_name } => {
                            self.objects.get_obj(other_name)?.position
                        }
                        Target::Mouse => self.mouse.position,
                    };
                    let target_vector = other + offset - self.objects[name].position;
                    let target_vector = target_vector
                        / (target_vector.x.powf(2.0) + target_vector.y.powf(2.0)).sqrt();
                    let move_to = |x: f32, other: f32, velocity: f32| {
                        if (x - other).abs() > velocity.abs() {
                            x + velocity
                        } else {
                            other
                        }
                    };
                    let velocity: Vec2 = target_vector * speed.as_value();

                    Vec2::new(
                        move_to(
                            self.objects[name].position.x,
                            other.x + offset.x,
                            velocity.x,
                        ),
                        move_to(
                            self.objects[name].position.y,
                            other.y + offset.y,
                            velocity.y,
                        ),
                    )
                };

                if let TargetType::StopWhenReached = target_type {
                    let other = match &target {
                        Target::Object { name: other_name } => {
                            self.objects.get_obj(other_name)?.position
                        }
                        Target::Mouse => self.mouse.position,
                    };
                    let close_enough =
                        |pos: f32, other: f32, offset: f32| (pos - (other + offset)).abs() < 0.5;
                    if close_enough(self.objects[name].position.x, other.x, offset.x)
                        && close_enough(self.objects[name].position.y, other.y, offset.y)
                    {
                        ActiveMotion::Stop
                    } else {
                        ActiveMotion::Target {
                            target,
                            target_type,
                            offset,
                            speed,
                        }
                    }
                } else {
                    ActiveMotion::Target {
                        target,
                        target_type,
                        offset,
                        speed,
                    }
                }
            }
            ActiveMotion::Accelerate {
                mut velocity,
                acceleration,
            } => {
                self.objects[name].position += velocity;
                velocity += acceleration;
                ActiveMotion::Accelerate {
                    velocity,
                    acceleration,
                }
            }
            ActiveMotion::Stop => ActiveMotion::Stop,
        };

        Ok(active_motion)
    }

    fn update_objects(&mut self) -> WeeResult<Vec<String>> {
        let mut played_sounds = Vec::new();

        let keys: Vec<String> = self.objects.keys().cloned().collect();
        for name in keys.iter() {
            let old_switch = self.objects[name].switch;

            self.objects[name].update_timer();

            let actions = self.check_triggers(name)?;

            self.apply_actions(name, &actions, &mut played_sounds)?;

            self.objects[name].update_animation();

            self.move_object(name)?;

            self.objects[name].update_switch(old_switch);
        }

        Ok(played_sounds)
    }

    pub fn has_been_won(&self) -> bool {
        match self.status.next_frame {
            WinStatus::Won | WinStatus::HasBeenWon => true,
            _ => false,
        }
    }

    pub fn update_and_render_frame(
        &mut self,
        renderer: &Renderer,
        event_pump: &mut EventPump,
    ) -> WeeResult<()> {
        self.frames.start_time = Instant::now();
        self.update_frame(event_pump, renderer.window.size())?;
        self.render_frame(renderer)?;
        self.sleep();

        Ok(())
    }
}

impl GameData {
    pub fn load(filename: &str) -> WeeResult<GameData> {
        let json_string = fs::read_to_string(filename)?;

        json_from_str(&json_string)
    }
}

pub trait RenderScene {
    fn draw_background(&self, background: &[BackgroundPart], images: &Images) -> WeeResult<()>;

    fn draw_objects(
        &self,
        objects: &Objects,
        images: &Images,
        drawn_over_text: &HashMap<String, Texture>,
    ) -> WeeResult<()>;
}

impl RenderScene for Renderer {
    fn draw_background(&self, background: &[BackgroundPart], images: &Images) -> WeeResult<()> {
        for part in background.iter() {
            match &part.sprite {
                Sprite::Image { name } => {
                    let texture = images.get_image(name)?;

                    self.prepare(&texture).set_dest(part.area.to_rect()).draw();
                }
                Sprite::Colour(colour) => {
                    let model = Model::new(part.area.to_rect(), None, 0.0, Flip::default());

                    self.fill_rectangle(model, *colour);
                }
            }
        }
        Ok(())
    }

    fn draw_objects(
        &self,
        objects: &Objects,
        images: &Images,
        drawn_over_text: &HashMap<String, Texture>,
    ) -> WeeResult<()> {
        let mut layers: Vec<u8> = objects.values().map(|o| o.layer).collect();
        layers.sort();
        layers.dedup();
        layers.reverse();
        for layer in layers.into_iter() {
            for (key, object) in objects.iter() {
                if object.layer == layer {
                    match &object.sprite {
                        Sprite::Image { name: image_name } => {
                            let texture = images.get_image(image_name)?;
                            let dest = object.rect();

                            self.prepare(&texture)
                                .set_dest(dest)
                                .set_angle(object.angle)
                                .set_origin(object.origin)
                                .flip(object.flip)
                                .draw();
                        }
                        Sprite::Colour(colour) => {
                            let model = Model::new(
                                Rect::new(
                                    object.position.x,
                                    object.position.y,
                                    object.size.width,
                                    object.size.height,
                                ),
                                object.origin,
                                object.angle,
                                object.flip,
                            );

                            self.fill_rectangle(model, *colour);
                        }
                    }
                    if drawn_over_text.contains_key(key) {
                        self.prepare(&drawn_over_text[key])
                            .set_dest(object.rect())
                            .set_angle(object.angle)
                            .set_origin(object.origin)
                            .flip(object.flip)
                            .draw();
                    }
                }
            }
        }
        Ok(())
    }
}
