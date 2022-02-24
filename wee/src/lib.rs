// TODO: Time 0 based vs 1 based?

//use macroquad::logging as log;

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_random::prelude::*;
    //use macroquad::logging as log;
    use std::{fs, iter::FromIterator};

    struct TestRng(RandomNumberGenerator);

    impl Default for TestRng {
        fn default() -> TestRng {
            TestRng(RandomNumberGenerator::new())
        }
    }

    impl WeeRng for TestRng {
        fn random_in_range(&mut self, min: f32, max: f32) -> f32 {
            self.0.range(min, max)
        }

        fn random_in_range_u32(&mut self, min: u32, max: u32) -> u32 {
            self.0.range(min, max)
        }

        fn random_in_slice<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T> {
            self.0.random_slice_entry(slice)
        }

        fn coin_flip(&mut self) -> bool {
            self.0.roll_dice(1, 2) == 1
        }
    }

    fn load_game_data(filename: impl AsRef<Path>) -> WeeResult<GameData> {
        let json_string = fs::read_to_string(&filename)?;

        fn json_from_str<'a, T: Deserialize<'a>>(text: &'a str) -> WeeResult<T> {
            match serde_json::from_str(text) {
                Ok(data) => Ok(data),
                Err(error) => Err(Box::new(error)),
            }
        }

        json_from_str(&json_string)
    }

    fn load_test_game(filename: &str, rng: &mut impl WeeRng) -> WeeResult<Game> {
        let game_data = load_game_data(filename)?;
        Ok(Game::from_data(game_data, rng)?)
    }

    fn play_test_game(
        game: &mut Game,
        inputs: &mut Vec<Mouse>,
        rng: &mut impl WeeRng,
    ) -> WeeResult<()> {
        while game.frames.remaining() != FrameCount::Frames(0) {
            let mouse = if inputs.len() > 0 {
                inputs.remove(0)
            } else {
                Mouse::default()
            };

            let world_actions = game.update_frame(mouse, rng)?;

            for action in &world_actions {
                if let WorldAction::EndEarly = action {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    #[test]
    fn triggers_time_start_on_first_frame() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());
        assert!(Trigger::Time(When::Start)
            .is_triggered(&game, "Simple", Mouse::default(), &mut TestRng::default())
            .unwrap());
    }

    #[test]
    fn triggers_time_end_on_last_frame() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());
        game.frames.ran += DEFAULT_GAME_LENGTH_IN_FRAMES - 1;
        assert!(Trigger::Time(When::End)
            .is_triggered(&game, "Simple", Mouse::default(), &mut TestRng::default())
            .unwrap());
    }

    #[test]
    fn only_triggers_time_end_on_last_frame() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());
        for _ in 0..DEFAULT_GAME_LENGTH_IN_FRAMES - 1 {
            assert!(!Trigger::Time(When::End)
                .is_triggered(&game, "Simple", Mouse::default(), &mut TestRng::default())
                .unwrap());
            game.frames.ran += 1;
        }
        assert!(Trigger::Time(When::End)
            .is_triggered(&game, "Simple", Mouse::default(), &mut TestRng::default())
            .unwrap());
    }

    #[test]
    fn triggers_time_end_on_first_frame_in_game_with_one_frame() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());
        game.frames.total = FrameCount::Frames(1);
        assert!(Trigger::Time(When::End)
            .is_triggered(&game, "Simple", Mouse::default(), &mut TestRng::default())
            .unwrap());
    }

    #[test]
    fn wins_game_with_every_frame_trigger() {
        let mut game = Game::default();
        let mut object = Object::default();
        object.instructions.push(Instruction {
            triggers: vec![],
            actions: vec![Action::Win],
        });
        game.objects.insert("Simple".to_string(), object);
        let actions =
            check_triggers(&game, "Simple", Mouse::default(), &mut TestRng::default()).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], Action::Win);
    }

    #[test]
    fn win_action_wins_game() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());

        Action::Win
            .apply(
                &mut game,
                "Simple",
                Mouse::default(),
                &mut TestRng::default(),
            )
            .unwrap();

        assert_eq!(game.status.next_frame, WinStatus::JustWon);
    }

    #[test]
    fn left_direction_moves_left() {
        let mut game = Game::default();
        game.objects.insert("Simple".to_string(), Object::default());

        game.objects["Simple"]
            .queued_motion
            .push(Motion::GoStraight {
                direction: MovementDirection::Direction {
                    possible_directions: HashSet::from_iter(vec![CompassDirection::Left]),
                },
                speed: Speed::Value(1.0),
            });

        move_object(
            &mut game,
            "Simple",
            Mouse::default(),
            &mut TestRng::default(),
        )
        .unwrap();

        assert_eq!(game.objects["Simple"].position.x, 799.0);
    }

    #[test]
    fn tenth_frame_set_switch_on() {
        let mut game = Game::default();
        let instructions = vec![Instruction {
            triggers: vec![Trigger::Time(When::Exact { time: 10 })],
            actions: vec![Action::SetProperty(PropertySetter::Switch(Switch::On))],
        }];
        let object = Object {
            instructions,
            ..Default::default()
        };
        game.objects.insert("Simple".to_string(), object);

        for _ in 0..11 {
            game.update_frame(Mouse::default(), &mut TestRng::default())
                .unwrap();
        }

        assert_eq!(game.objects["Simple"].switch, SwitchState::SwitchedOn);
    }

    #[test]
    fn check_all_saved_runs() {
        let mut wrong_result_games = Vec::new();

        for entry in fs::read_dir("saved-runs").unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                let saved_run: SavedRun = bincode::deserialize(&fs::read(&path).unwrap()).unwrap();
                let game_path = Path::new("../main-game").join(saved_run.path);

                let mut rng = TestRng(RandomNumberGenerator::seeded(saved_run.seed));
                let mut inputs = saved_run.inputs;

                println!(
                    "path: {:?}\n, difficulty: {}\n, seed: {}\n, won: {}\n",
                    game_path, saved_run.difficulty, saved_run.seed, saved_run.has_been_won
                );

                let mut game = load_test_game(&game_path.to_str().unwrap(), &mut rng).unwrap();

                play_test_game(&mut game, &mut inputs, &mut rng).unwrap();
                if (game.status.current == WinStatus::Won
                    || game.status.current == WinStatus::JustWon)
                    != saved_run.has_been_won
                {
                    wrong_result_games.push(path.to_str().unwrap().to_string());
                }
            }
        }
        assert_eq!(wrong_result_games, Vec::<&str>::new());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedRun {
    pub path: String,
    pub inputs: Vec<Mouse>,
    pub difficulty: u32,
    pub seed: u64,
    pub has_been_won: bool,
}

pub trait PathToString {
    fn to_string(&self) -> WeeResult<String>;
}

impl PathToString for Path {
    fn to_string(&self) -> WeeResult<String> {
        let s = self
            .to_str()
            .map(|s| s.to_string())
            .ok_or("Cannot convert to string")?;
        Ok(s)
    }
}

const FPS: f32 = 60.0;
const DEFAULT_GAME_LENGTH_IN_FRAMES: u32 = 60 * 4;

impl Game {
    pub fn from_data(game_data: GameData, rng: &mut impl WeeRng) -> WeeResult<Game> {
        Ok(Game {
            objects: Objects::from_serialised(game_data.objects, rng),
            background: game_data.background,
            frames: FrameInfo::from_length(game_data.length),
            status: GameStatus {
                current: WinStatus::NotYetWon,
                next_frame: WinStatus::NotYetWon,
            },
            intro_text: game_data.intro_text.as_deref().unwrap_or("").to_string(),
            effect: Effect::None,
            difficulty: 1,
        })
    }

    fn update_win_status(&mut self) {
        self.status.current = self.status.next_frame;
        self.status.next_frame = match self.status.next_frame {
            WinStatus::JustWon => WinStatus::Won,
            WinStatus::JustLost => WinStatus::Lost,
            _ => self.status.next_frame,
        };
    }

    pub fn update_frame(
        &mut self,
        mouse: Mouse,
        rng: &mut impl WeeRng,
    ) -> WeeResult<Vec<WorldAction>> {
        // TODO: Optimise this line by doing it only at the start of the game
        let keys: Vec<String> = self.objects.keys().cloned().collect();

        let mut world_actions = Vec::new();
        if self.effect == Effect::Freeze {
            for name in keys.iter() {
                self.objects[name].update_timer();
                let actions = check_triggers(self, name, mouse, rng)?;
                for action in actions {
                    if action == Action::EndEarly {
                        world_actions.push(WorldAction::EndEarly);
                    }
                }
            }
        } else {
            for name in keys.iter() {
                let old_switch = self.objects[name].switch;
                self.objects[name].update_timer();
                let actions = check_triggers(self, name, mouse, rng)?;
                for action in actions {
                    let mut new_world_actions = action.apply(self, name, mouse, rng)?;
                    world_actions.append(&mut new_world_actions);
                }
                if let Some(sprite) = self.objects[name].animation.update() {
                    self.objects[name].sprite = sprite;
                }
                move_object(self, name, mouse, rng)?;
                self.objects[name].update_switch(old_switch);
            }
        }

        self.update_win_status();

        self.frames.ran += 1;

        Ok(world_actions)
    }
}

impl Default for Game {
    fn default() -> Game {
        Game {
            objects: Objects::new(),
            background: Vec::new(),
            frames: FrameInfo::default(),
            status: GameStatus::default(),
            intro_text: "".to_string(),
            effect: Effect::None,
            difficulty: 1,
        }
    }
}

impl Default for Object {
    fn default() -> Object {
        Object {
            sprite: Sprite::Colour(Colour::black()),
            position: Vec2::new(800.0, 450.0),
            size: Size::new(100.0, 100.0),
            angle: 0.0,
            origin: None,
            collision_area: None,
            flip: Flip::default(),
            layer: 0,
            switch: SwitchState::Off,
            instructions: Vec::new(),
            queued_motion: Vec::new(),
            active_motion: ActiveMotion::Stop,
            timer: None,
            animation: AnimationStatus::None,
        }
    }
}

pub trait ObjectList {
    fn get_obj(&self, name: &str) -> WeeResult<&Object>;

    fn from_serialised(objects: Vec<SerialiseObject>, rng: &mut impl WeeRng) -> Self;
}

impl ObjectList for Objects {
    fn get_obj(&self, name: &str) -> WeeResult<&Object> {
        self.get(name)
            .ok_or_else(|| format!("Couldn't find object with name {}", name).into())
    }

    fn from_serialised(objects: Vec<SerialiseObject>, rng: &mut impl WeeRng) -> Objects {
        let mut new_objects = Objects::new();

        for object in objects {
            new_objects.insert(object.name.clone(), object.into_object(rng));
        }

        new_objects
    }
}

impl Trigger {
    fn is_triggered(
        &self,
        game: &Game,
        name: &str,
        mouse: Mouse,
        rng: &mut impl WeeRng,
    ) -> WeeResult<bool> {
        let is_point_in_area = |pos: Vec2, area: AABB| {
            pos.x >= area.min.x && pos.y >= area.min.y && pos.x < area.max.x && pos.y < area.max.y
        };
        let is_mouse_in_area = |mouse: Mouse, area| is_point_in_area(mouse.position, area);
        let c2v = |v: Vec2| c2::Vec2::new(v.x, v.y);
        let object = game.objects.get(name).ok_or("Couldn't find object")?;

        let triggered = match self {
            Trigger::Time(When::Start) => game.frames.ran == 0,
            Trigger::Time(When::End) => game.frames.is_final(),
            Trigger::Time(When::Exact { time }) => game.frames.ran == *time,
            Trigger::Time(When::Random { .. }) => false,
            Trigger::Collision(CollisionWith::Object { name: other_name }) => {
                let other_obj = game.objects.get_obj(other_name)?;

                object.poly().collides_with(&other_obj.poly())
            }
            Trigger::Collision(CollisionWith::Area(area)) => {
                let area = c2::AABB::new(c2v(area.min), c2v(area.max));

                object.poly().collides_with(&area)
            }
            Trigger::WinStatus(win_status) => match win_status {
                WinStatus::Won => {
                    matches!(game.status.current, WinStatus::Won | WinStatus::JustWon)
                }
                WinStatus::Lost => {
                    matches!(game.status.current, WinStatus::Lost | WinStatus::JustLost)
                }
                WinStatus::NotYetLost => matches!(
                    game.status.current,
                    WinStatus::NotYetLost
                        | WinStatus::NotYetWon
                        | WinStatus::JustWon
                        | WinStatus::Won
                ),
                WinStatus::NotYetWon => matches!(
                    game.status.current,
                    WinStatus::NotYetWon
                        | WinStatus::NotYetLost
                        | WinStatus::JustLost
                        | WinStatus::Lost
                ),
                _ => game.status.current == *win_status,
            },
            Trigger::Input(Input::Mouse { over, interaction }) => {
                let is_over = match over {
                    MouseOver::Object { name: other_name } => {
                        let other_obj = game.objects.get_obj(other_name)?;
                        other_obj
                            .poly()
                            .gjk(&c2::Circle::new(c2v(mouse.position), 1.0))
                            .use_radius(false)
                            .run()
                            .distance()
                            == 0.0
                    }
                    MouseOver::Area(area) => is_mouse_in_area(mouse, *area),
                    MouseOver::Anywhere => true,
                };
                is_over
                    && match interaction {
                        MouseInteraction::Button { state } => *state == mouse.state,
                        MouseInteraction::Hover => true,
                    }
            }
            Trigger::CheckProperty {
                name: object_name,
                check,
            } => {
                let obj = game.objects.get_obj(object_name)?;
                match check {
                    PropertyCheck::Switch(switch_state) => {
                        if *switch_state == SwitchState::Off {
                            obj.switch == SwitchState::SwitchedOff || obj.switch == SwitchState::Off
                        } else if *switch_state == SwitchState::On {
                            obj.switch == SwitchState::SwitchedOn || obj.switch == SwitchState::On
                        } else {
                            obj.switch == *switch_state
                        }
                    }
                    PropertyCheck::Sprite(sprite) => obj.sprite == *sprite,
                    PropertyCheck::FinishedAnimation => {
                        matches!(obj.animation, AnimationStatus::Finished)
                    }
                    PropertyCheck::Timer => match obj.timer {
                        Some(alarm) => alarm == 0,
                        None => false,
                    },
                }
            }
            Trigger::Random { chance } => {
                let roll = rng.random_in_range(0.0, 1.0);
                roll < *chance
            }
            Trigger::DifficultyLevel { levels } => levels.contains(&game.difficulty),
        };
        Ok(triggered)
    }
}
trait AllOk: Iterator {
    fn all_ok<F>(&mut self, f: F) -> WeeResult<bool>
    where
        Self: Sized,
        F: FnMut(Self::Item) -> WeeResult<bool>,
    {
        Ok(self
            .map(f)
            .collect::<WeeResult<Vec<bool>>>()?
            .into_iter()
            .all(|t| t))
    }
}

impl<T: ?Sized> AllOk for T where T: Iterator {}

fn check_triggers(
    game: &Game,
    name: &str,
    mouse: Mouse,
    rng: &mut impl WeeRng,
) -> WeeResult<Vec<Action>> {
    let mut actions = Vec::new();
    for instruction in game.objects[name].instructions.iter() {
        if instruction
            .triggers
            .iter()
            .all_ok(|trigger| trigger.is_triggered(game, name, mouse, rng))?
        {
            actions.extend(instruction.actions.clone())
        }
    }
    Ok(actions)
}

impl SerialiseObject {
    pub fn replace_text(&mut self, text_replacements: &[(&str, String)]) {
        fn replace_text_in_action(action: &mut Action, text_replacements: &[(&str, String)]) {
            if let Action::DrawText { text, .. } = action {
                for (before, after) in text_replacements {
                    *text = text.replace(before, &after);
                }
            } else if let Action::Random { random_actions } = action {
                for action in random_actions {
                    replace_text_in_action(action, text_replacements);
                }
            }
        }

        for instruction in self.instructions.iter_mut() {
            for action in instruction.actions.iter_mut() {
                replace_text_in_action(action, &text_replacements);
            }
        }
    }

    pub fn into_object(self, rng: &mut impl WeeRng) -> Object {
        let switch = self.switch.to_switch_state();

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
            ..Default::default()
        };
        for instruction in object.instructions.iter_mut() {
            for trigger in instruction.triggers.iter_mut() {
                if let Trigger::Time(When::Random { start, end }) = trigger {
                    *trigger = Trigger::Time(When::Exact {
                        // TODO: Sanify this
                        time: rng.random_in_range_u32(*start, *end + 1),
                    });
                }
            }
        }

        object
    }
}

impl Default for FrameInfo {
    fn default() -> FrameInfo {
        FrameInfo {
            total: FrameCount::Frames(DEFAULT_GAME_LENGTH_IN_FRAMES),
            ran: 0,
            steps_taken: 0,
            to_run: 0,
            total_time_elapsed: 0.0,
            previous_frame_time: 0.0,
        }
    }
}

impl FrameInfo {
    fn from_length(length: Length) -> FrameInfo {
        let total_frames = match length {
            Length::Seconds(seconds) => FrameCount::Frames((seconds * FPS) as u32),
            Length::Infinite => FrameCount::Infinite,
        };

        FrameInfo {
            total: total_frames,
            ran: 0,
            steps_taken: 0,
            to_run: 0,
            total_time_elapsed: 0.0,
            previous_frame_time: 0.0,
        }
    }

    pub fn remaining(self) -> FrameCount {
        match self.total {
            FrameCount::Frames(frames) => {
                if self.ran > frames {
                    FrameCount::Frames(0)
                } else {
                    FrameCount::Frames(frames - self.ran)
                }
            }
            FrameCount::Infinite => FrameCount::Infinite,
        }
    }

    pub fn is_final(self) -> bool {
        self.remaining() == FrameCount::Frames(1)
    }
}

impl Default for GameStatus {
    fn default() -> GameStatus {
        GameStatus {
            current: WinStatus::NotYetWon,
            next_frame: WinStatus::NotYetWon,
        }
    }
}

impl Switch {
    fn to_switch_state(self) -> SwitchState {
        match self {
            Switch::On => SwitchState::On,
            Switch::Off => SwitchState::Off,
        }
    }
}

impl Object {
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
            *point = c2v(
                point.x() * c - point.y() * s + origin.x,
                point.x() * s + point.y() * c + origin.y,
            );
        }
        c2::Poly::from_slice(&points)
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

    pub fn origin(&self) -> Vec2 {
        self.origin
            .unwrap_or_else(|| Vec2::new(self.half_width(), self.half_height()))
    }

    pub fn origin_in_world(&self) -> Vec2 {
        self.top_left() + self.origin()
    }

    pub fn half_width(&self) -> f32 {
        self.size.width / 2.0
    }

    pub fn half_height(&self) -> f32 {
        self.size.height / 2.0
    }

    pub fn top_left(&self) -> Vec2 {
        Vec2::new(
            self.position.x - self.half_width(),
            self.position.y - self.half_height(),
        )
    }

    fn bottom_right(&self) -> Vec2 {
        Vec2::new(
            self.position.x + self.half_width(),
            self.position.y + self.half_height(),
        )
    }

    pub fn trig_angle(&self) -> f32 {
        (self.angle - 90.0).to_radians()
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

    pub fn update_timer(&mut self) {
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
}

pub enum WorldAction {
    StopMusic,
    PlaySound { name: String },
    DrawText { name: String, text: DrawnText },
    EndEarly,
}

#[derive(Debug, Clone)]
pub struct DrawnText {
    pub text: String,
    pub font: String,
    pub colour: Colour,
    //resize: TextResize,
    pub justify: JustifyText,
}

pub trait WeeRng {
    fn random_in_range(&mut self, min: f32, max: f32) -> f32;
    fn random_in_range_u32(&mut self, min: u32, max: u32) -> u32;
    fn random_in_slice<'a, T>(&mut self, slice: &'a [T]) -> Option<&'a T>;
    fn coin_flip(&mut self) -> bool;
}

impl Action {
    fn apply(
        &self,
        game: &mut Game,
        name: &str,
        mouse: Mouse,
        rng: &mut impl WeeRng,
    ) -> WeeResult<Vec<WorldAction>> {
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
            try_to_set_status(status, WinStatus::JustLost, WinStatus::JustWon);
        };
        let try_to_lose = |status| {
            try_to_set_status(status, WinStatus::JustWon, WinStatus::JustLost);
        };
        let mut world_actions = Vec::new();
        match self {
            Action::Motion(motion) => {
                game.objects[name].queued_motion.push(motion.clone());
            }
            Action::Win => {
                try_to_win(&mut game.status);
            }
            Action::Lose => {
                try_to_lose(&mut game.status);
            }
            Action::Effect(new_effect) => {
                game.effect = *new_effect;
            }
            Action::PlaySound { name: sound_name } => {
                world_actions.push(WorldAction::PlaySound {
                    name: sound_name.clone(),
                });
            }
            Action::StopMusic => {
                world_actions.push(WorldAction::StopMusic);
            }
            Action::Animate {
                animation_type,
                sprites,
                speed,
            } => {
                game.objects[name].animation =
                    AnimationStatus::start(*animation_type, sprites, *speed);

                if let Some(sprite) = sprites.get(0).cloned() {
                    game.objects[name].sprite = sprite;
                }
            }
            Action::DrawText {
                text,
                font,
                colour,
                resize: _resize,
                justify,
            } => {
                world_actions.push(WorldAction::DrawText {
                    name: name.to_string(),
                    text: DrawnText {
                        text: text.clone(),
                        font: font.clone(),
                        colour: *colour,
                        justify: *justify,
                    },
                });
            }
            Action::SetProperty(PropertySetter::Angle(angle_setter)) => {
                game.objects[name].angle = match angle_setter {
                    AngleSetter::Value(value) => *value,
                    AngleSetter::Increase(value) => game.objects[name].angle + value,
                    AngleSetter::Decrease(value) => game.objects[name].angle - value,
                    AngleSetter::Match { name: other_name } => {
                        game.objects.get_obj(other_name)?.angle
                    }
                    AngleSetter::Clamp { min, max } => {
                        let mut angle = game.objects[name].angle;
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
                    AngleSetter::RotateToObject { name: other_name } => {
                        let other_centre = game.objects.get_obj(other_name)?.position;
                        let centre = game.objects[name].origin_in_world();
                        (other_centre.y - centre.y)
                            .atan2(other_centre.x - centre.x)
                            .to_degrees()
                            + 90.0
                    }
                    AngleSetter::RotateToMouse => {
                        let centre = game.objects[name].origin_in_world();
                        let error = 0.00001;
                        if (centre.x - mouse.position.x).abs() < error
                            && (centre.y - mouse.position.y).abs() < error
                        {
                            game.objects[name].angle
                        } else {
                            (mouse.position.y - centre.y)
                                .atan2(mouse.position.x - centre.x)
                                .to_degrees()
                                + 90.0
                        }
                    }
                };
            }
            Action::SetProperty(PropertySetter::Sprite(sprite)) => {
                game.objects[name].sprite = sprite.clone();
                game.objects[name].animation = AnimationStatus::None;
            }
            Action::SetProperty(PropertySetter::Size(size_setter)) => {
                let old_size = game.objects[name].size;
                game.objects[name].size = match size_setter {
                    SizeSetter::Value(value) => *value,
                    SizeSetter::Grow(SizeDifference::Value(value)) => Size {
                        width: game.objects[name].size.width + value.width,
                        height: game.objects[name].size.height + value.height,
                    },
                    SizeSetter::Shrink(SizeDifference::Value(value)) => Size {
                        width: game.objects[name].size.width - value.width,
                        height: game.objects[name].size.height - value.height,
                    },
                    SizeSetter::Grow(SizeDifference::Percent(percent)) => {
                        let w = game.objects[name].size.width;
                        let h = game.objects[name].size.height;
                        Size {
                            width: w + w * (percent.width / 100.0),
                            height: h + h * (percent.height / 100.0),
                        }
                    }
                    SizeSetter::Shrink(SizeDifference::Percent(percent)) => {
                        let w = game.objects[name].size.width;
                        let h = game.objects[name].size.height;
                        Size {
                            width: w - w * (percent.width / 100.0),
                            height: h - h * (percent.height / 100.0),
                        }
                    }
                    SizeSetter::Clamp { min, max } => Size {
                        width: game.objects[name].size.width.min(max.width).max(min.width),
                        height: game.objects[name]
                            .size
                            .height
                            .min(max.height)
                            .max(min.height),
                    },
                };
                let size = game.objects[name].size;
                let difference =
                    Size::new(size.width / old_size.width, size.height / old_size.height);
                match &mut game.objects[name].collision_area {
                    Some(area) => {
                        *area = AABB {
                            min: Vec2::new(
                                if old_size.width == 0.0 {
                                    size.width
                                } else {
                                    area.min.x * difference.width
                                },
                                if old_size.height == 0.0 {
                                    size.height
                                } else {
                                    area.min.y * difference.height
                                },
                            ),
                            max: Vec2::new(
                                if old_size.width == 0.0 {
                                    size.width
                                } else {
                                    area.max.x * difference.width
                                },
                                if old_size.height == 0.0 {
                                    size.height
                                } else {
                                    area.max.y * difference.width
                                },
                            ),
                        };
                    }
                    None => {}
                }
            }
            Action::SetProperty(PropertySetter::Switch(switch)) => {
                if *switch == Switch::On && game.objects[name].switch != SwitchState::On {
                    game.objects[name].switch = SwitchState::SwitchedOn;
                } else if *switch == Switch::Off && game.objects[name].switch != SwitchState::Off {
                    game.objects[name].switch = SwitchState::SwitchedOff;
                }
            }
            Action::SetProperty(PropertySetter::Timer { time }) => {
                game.objects[name].timer = Some(*time);
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::Flip)) => {
                game.objects[name].flip.horizontal = !game.objects[name].flip.horizontal;
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::Flip)) => {
                game.objects[name].flip.vertical = !game.objects[name].flip.vertical;
            }
            Action::SetProperty(PropertySetter::FlipHorizontal(FlipSetter::SetFlip(flipped))) => {
                game.objects[name].flip.horizontal = *flipped;
            }
            Action::SetProperty(PropertySetter::FlipVertical(FlipSetter::SetFlip(flipped))) => {
                game.objects[name].flip.vertical = *flipped;
            }
            Action::SetProperty(PropertySetter::Layer(layer_setter)) => {
                game.objects[name].layer = match layer_setter {
                    LayerSetter::Value(value) => *value,
                    LayerSetter::Increase => {
                        game.objects[name].layer
                            + if game.objects[name].layer < std::u8::MAX - 1 {
                                1
                            } else {
                                0
                            }
                    }
                    LayerSetter::Decrease => {
                        game.objects[name].layer - if game.objects[name].layer > 0 { 1 } else { 0 }
                    }
                };
            }
            Action::Random { random_actions } => {
                let action = rng.random_in_slice(random_actions);
                if let Some(action) = action {
                    return action.apply(game, name, mouse, rng);
                }
            }
            Action::EndEarly => {
                world_actions.push(WorldAction::EndEarly);
            }
        };

        Ok(world_actions)
    }
}

impl Default for Mouse {
    fn default() -> Mouse {
        Mouse {
            position: Vec2::new(PROJECTION_WIDTH / 2.0, PROJECTION_HEIGHT / 2.0),
            state: ButtonState::Up,
        }
    }
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

    pub fn to_animation_time(self) -> u32 {
        match self {
            Speed::VerySlow => 32,
            Speed::Slow => 16,
            Speed::Normal => 8,
            Speed::Fast => 4,
            Speed::VeryFast => 2,
            Speed::Value(value) => value as u32,
        }
    }
}

impl AnimationStatus {
    pub fn start(
        animation_type: AnimationType,
        sprites: &[Sprite],
        speed: Speed,
    ) -> AnimationStatus {
        let should_loop = match animation_type {
            AnimationType::Loop => true,
            AnimationType::PlayOnce => false,
        };
        AnimationStatus::Animating(Animation {
            should_loop,
            sprites: sprites.to_vec(),
            index: 0,
            speed,
            time_to_next_change: speed.to_animation_time(),
        })
    }

    pub fn update(&mut self) -> Option<Sprite> {
        match self {
            AnimationStatus::Animating(animation) => {
                if animation.time_to_next_change == 0 {
                    if animation.sprites.is_empty() {
                    } else if animation.index == animation.sprites.len() - 1 {
                        if animation.should_loop {
                            animation.index = 0;
                            animation.time_to_next_change = animation.speed.to_animation_time();
                            return Some(animation.sprites[0].clone());
                        } else {
                            *self = AnimationStatus::Finished;
                        }
                    } else {
                        animation.index += 1;
                        animation.time_to_next_change = animation.speed.to_animation_time();
                        return Some(animation.sprites[animation.index].clone());
                    }
                } else {
                    animation.time_to_next_change -= 1;
                }
            }
            AnimationStatus::Finished => {
                *self = AnimationStatus::None;
            }
            AnimationStatus::None => {}
        };

        None
    }
}

fn move_object(game: &mut Game, name: &str, mouse: Mouse, rng: &mut impl WeeRng) -> WeeResult<()> {
    let mut clamps = Vec::new();
    for mut motion in game.objects[name].queued_motion.clone().into_iter() {
        if let Motion::JumpTo(JumpLocation::ClampPosition { .. }) = &motion {
        } else {
            for area in clamps {
                clamp_position(&mut game.objects[name].position, area);
            }
            clamps = Vec::new();
        }
        game.objects[name].active_motion = match &mut motion {
            Motion::GoStraight { direction, speed } => {
                let velocity = direction.to_vector(&game.objects[name], *speed, rng);
                ActiveMotion::GoStraight { velocity }
            }
            Motion::JumpTo(jump_location) => {
                match jump_location {
                    JumpLocation::Point(point) => {
                        game.objects[name].position = *point;
                    }
                    JumpLocation::Relative { to, distance } => match to {
                        RelativeTo::CurrentPosition => {
                            game.objects[name].position += *distance;
                        }
                        RelativeTo::CurrentAngle => {
                            let angle = game.objects[name].trig_angle();
                            game.objects[name].position.x +=
                                -distance.y * angle.cos() - distance.x * angle.sin();
                            game.objects[name].position.y +=
                                -distance.y * angle.sin() + distance.x * angle.cos();
                        }
                    },
                    JumpLocation::Area(area) => {
                        fn gen_in_area(area: AABB, rng: &mut impl WeeRng) -> Vec2 {
                            Vec2::new(
                                rng.gen_in_unordered_range(area.min.x, area.max.x),
                                rng.gen_in_unordered_range(area.min.y, area.max.y),
                            )
                        }
                        game.objects[name].position = gen_in_area(*area, rng);
                    }
                    JumpLocation::ClampPosition { .. } => {
                        //clamp_position(&mut game.objects[name].position, *area);
                    }
                    JumpLocation::Object { name: other_name } => {
                        game.objects[name].position = game.objects.get_obj(&other_name)?.position;
                    }
                    JumpLocation::Mouse => {
                        game.objects[name].position = mouse.position;
                    }
                }
                if let Motion::JumpTo(JumpLocation::ClampPosition { area }) = motion {
                    clamps.push(area);
                    game.objects[name].active_motion.clone()
                } else {
                    ActiveMotion::Stop
                }
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
                                    Angle::Current => game.objects[name].angle,
                                    Angle::Degrees(degrees) => *degrees,
                                    Angle::Random { min, max } => {
                                        rng.gen_in_unordered_range(*min, *max)
                                    }
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
                                let enough_horizontal_space = width < game.objects[name].size.width;
                                let enough_vertical_space = height < game.objects[name].size.height;

                                let mut possible_directions = if !possible_directions.is_empty() {
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
                                possible_directions.sort();
                                let dir = rng.random_in_slice(&possible_directions);
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
                        velocity: rng.random_velocity(*speed),
                    },
                    MovementType::Bounce { initial_direction } => {
                        let frames_in_bounce = 60.0 * Speed::Normal.as_value() / speed.as_value();
                        let acceleration = -2.0 * (area.min.y - area.max.y)
                            / (frames_in_bounce * frames_in_bounce);
                        let velocity = {
                            let y_velocity = 2.0 * (area.min.y - game.objects[name].position.y)
                                / frames_in_bounce;
                            Vec2::new(0.0, y_velocity)
                        };
                        let direction = initial_direction.clone().unwrap_or_else(|| {
                            if rng.coin_flip() {
                                BounceDirection::Left
                            } else {
                                BounceDirection::Right
                            }
                        });

                        ActiveRoam::Bounce {
                            velocity,
                            direction,
                            acceleration,
                            frames_in_bounce,
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
                game.objects.get_obj(&other_name)?;
                let temp = game.objects[&*other_name].position;
                game.objects[&*other_name].position = game.objects[name].position;
                game.objects[name].position = temp;
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
            Motion::Accelerate(Acceleration::Continuous { direction, speed }) => {
                let speed = Speed::Value(speed.as_value() / 40.0);
                let acceleration = direction.to_vector(&game.objects[name], speed, rng);
                let velocity = match &game.objects[name].active_motion {
                    ActiveMotion::Accelerate { velocity, .. } => *velocity,
                    ActiveMotion::GoStraight { velocity } => *velocity,
                    ActiveMotion::Roam { movement_type, .. } => match movement_type {
                        ActiveRoam::Insect { velocity } => *velocity,
                        ActiveRoam::Bounce { velocity, .. } => *velocity,
                        ActiveRoam::Reflect { velocity, .. } => *velocity,
                        _ => Vec2::zero(),
                    },
                    ActiveMotion::Target { .. } => Vec2::zero(),
                    ActiveMotion::SlowDown { velocity, .. } => *velocity,
                    ActiveMotion::Stop => Vec2::zero(),
                };
                ActiveMotion::Accelerate {
                    velocity,
                    acceleration,
                }
            }
            Motion::Accelerate(Acceleration::SlowDown { speed }) => {
                let velocity = match &game.objects[name].active_motion {
                    ActiveMotion::Accelerate { velocity, .. } => *velocity,
                    ActiveMotion::GoStraight { velocity } => *velocity,
                    ActiveMotion::Roam { movement_type, .. } => match movement_type {
                        ActiveRoam::Insect { velocity } => *velocity,
                        ActiveRoam::Bounce { velocity, .. } => *velocity,
                        ActiveRoam::Reflect { velocity, .. } => *velocity,
                        _ => Vec2::zero(),
                    },
                    ActiveMotion::Target { .. } => Vec2::zero(),
                    ActiveMotion::SlowDown { velocity, .. } => *velocity,
                    ActiveMotion::Stop => Vec2::zero(),
                };
                if velocity.x == 0.0 && velocity.y == 0.0 {
                    ActiveMotion::Stop
                } else {
                    let deceleration = -(velocity.unit() * (speed.as_value() / 40.0));
                    ActiveMotion::SlowDown {
                        velocity,
                        deceleration,
                    }
                }
            }
            Motion::Stop => ActiveMotion::Stop,
        };
    }
    game.objects[name].queued_motion = Vec::new();

    game.objects[name].active_motion = update_active_motion(game, name, mouse, rng)?;

    for area in clamps {
        clamp_position(&mut game.objects[name].position, area);
        game.objects[name].active_motion = ActiveMotion::Stop;
    }

    Ok(())
}

trait ConcreteRng: WeeRng {
    fn random_velocity(&mut self, speed: Speed) -> Vec2 {
        let speed = speed.as_value();
        Vec2::new(
            self.random_in_range(-speed, speed),
            self.random_in_range(-speed, speed),
        )
    }

    fn gen_in_unordered_range(&mut self, min: f32, max: f32) -> f32 {
        if min > max {
            self.random_in_range(max, min)
        } else if max > min {
            self.random_in_range(min, max)
        } else {
            min
        }
    }
}

impl<T> ConcreteRng for T where T: WeeRng {}

pub fn clamp_position(position: &mut Vec2, area: AABB) {
    position.x = position.x.min(area.max.x).max(area.min.x);
    position.y = position.y.min(area.max.y).max(area.min.y);
}

fn vector_from_angle(angle: f32, speed: Speed) -> Vec2 {
    let speed = speed.as_value();
    let angle = (angle - 90.0).to_radians();
    Vec2::new(speed * angle.cos(), speed * angle.sin())
}

fn angle_from_direction(
    direction: &MovementDirection,
    object: &Object,
    rng: &mut impl WeeRng,
) -> f32 {
    match direction {
        MovementDirection::Angle(angle) => match angle {
            Angle::Current => object.angle,
            Angle::Degrees(degrees) => *degrees,
            Angle::Random { min, max } => rng.gen_in_unordered_range(*min, *max),
        },
        MovementDirection::Direction {
            possible_directions,
        } => {
            let mut possible_directions = if !possible_directions.is_empty() {
                possible_directions.iter().cloned().collect()
            } else {
                CompassDirection::all_directions()
            };
            possible_directions.sort();
            let dir = rng.random_in_slice(&possible_directions).unwrap();
            dir.angle()
        }
    }
}

impl MovementDirection {
    fn angle(&self, object: &Object, rng: &mut impl WeeRng) -> f32 {
        angle_from_direction(self, object, rng)
    }
    pub fn to_vector(&self, object: &Object, speed: Speed, rng: &mut impl WeeRng) -> Vec2 {
        vector_from_angle(self.angle(object, rng), speed)
    }
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

fn update_active_motion(
    game: &mut Game,
    name: &str,
    mouse: Mouse,
    rng: &mut impl WeeRng,
) -> WeeResult<ActiveMotion> {
    let active_motion = match game.objects[name].active_motion.clone() {
        ActiveMotion::GoStraight { velocity } => {
            game.objects[name].position += velocity;
            ActiveMotion::GoStraight { velocity }
        }
        ActiveMotion::Roam {
            movement_type,
            area,
            speed,
        } => {
            let movement_type = match movement_type {
                ActiveRoam::Wiggle => {
                    game.objects[name].position += rng.random_velocity(speed);
                    clamp_position(&mut game.objects[name].position, area);

                    ActiveRoam::Wiggle
                }
                ActiveRoam::Insect { mut velocity } => {
                    const CHANGE_DIRECTION_PROBABILTY: f32 = 0.1;
                    if rng.random_in_range(0.0, 1.0) < CHANGE_DIRECTION_PROBABILTY {
                        velocity = rng.random_velocity(speed);
                    }
                    game.objects[name].position += velocity;

                    clamp_position(&mut game.objects[name].position, area);

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
                                    if manifold.count() > 0 {
                                        let depth = manifold.depths()[0];
                                        if depth > longest_depth || closest_manifold.is_none() {
                                            closest_manifold = Some(manifold);
                                            position = objects[other_name].position;
                                            longest_depth = depth;
                                        }
                                    }
                                }
                            }
                            (closest_manifold, position)
                        }
                        let (original_manifold, other_position) = calculate_closest_manifold(
                            &game.objects,
                            name,
                            game.objects[name].poly(),
                        );
                        let move_away = |manifold: Option<c2::Manifold>| {
                            if let Some(manifold) = manifold {
                                let normal = manifold.normal();
                                let depth = manifold.depths()[0];
                                Vec2::new(normal.x() * depth, normal.y() * depth)
                            } else {
                                Vec2::zero()
                            }
                        };
                        game.objects[name].position -= move_away(original_manifold);

                        let closest_manifold = {
                            let moved_poly = {
                                let transformation = c2::Transformation::new(
                                    [velocity.x, velocity.y],
                                    c2::Rotation::zero(),
                                );
                                (game.objects[name].poly(), transformation)
                            };

                            let (new_manifold, _) =
                                calculate_closest_manifold(&game.objects, name, moved_poly);

                            let is_moving_towards = {
                                let to_point = other_position - game.objects[name].position;
                                (to_point.x * velocity.x + to_point.y * velocity.y) > 0.0
                            };
                            if new_manifold.is_none() && is_moving_towards {
                                original_manifold
                            } else {
                                new_manifold
                            }
                        };
                        game.objects[name].position -= move_away(closest_manifold);
                        if let Some(manifold) = closest_manifold {
                            let normal = manifold.normal();
                            let len = (normal.x().powf(2.0) + normal.y().powf(2.0)).sqrt();
                            if len != 0.0 {
                                let normal = Vec2::new(normal.x() / len, normal.y() / len);
                                velocity = velocity - 2.0 * (velocity * normal) * normal;
                            }
                        }
                    }
                    if game.objects[name].position.x + velocity.x < area.min.x {
                        velocity.x = velocity.x.abs();
                    }
                    if game.objects[name].position.x + velocity.x > area.max.x {
                        velocity.x = -velocity.x.abs();
                    }
                    if game.objects[name].position.y + velocity.y < area.min.y {
                        velocity.y = velocity.y.abs();
                    }
                    if game.objects[name].position.y + velocity.y > area.max.y {
                        velocity.y = -velocity.y.abs();
                    }
                    game.objects[name].position += velocity;

                    ActiveRoam::Reflect {
                        velocity,
                        movement_handling,
                    }
                }
                ActiveRoam::Bounce {
                    mut velocity,
                    mut direction,
                    acceleration,
                    frames_in_bounce,
                } => {
                    let (x, y) = (game.objects[name].position.x, game.objects[name].position.y);
                    if y < area.min.y && velocity.y < 0.0 {
                        velocity.y = 0.0;
                    }
                    if y < area.max.y {
                        velocity.y += acceleration;
                    } else if y > area.max.y {
                        velocity.y = -acceleration * frames_in_bounce;
                    }
                    if x > area.max.x {
                        direction = BounceDirection::Left;
                    } else if x < area.min.x {
                        direction = BounceDirection::Right;
                    }
                    if x >= area.min.x
                        && x <= area.max.x
                        && game.objects[name].size.width >= area.width()
                    {
                        velocity.x = 0.0;
                    } else {
                        match direction {
                            BounceDirection::Left => velocity.x = -speed.as_value() / 2.0,
                            BounceDirection::Right => velocity.x = speed.as_value() / 2.0,
                        }
                    }
                    game.objects[name].position += velocity;

                    ActiveRoam::Bounce {
                        velocity,
                        direction,
                        acceleration,
                        frames_in_bounce,
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
            game.objects[name].position = {
                let other = match &target {
                    Target::Object { name: other_name } => {
                        game.objects.get_obj(other_name)?.position
                    }
                    Target::Mouse => mouse.position,
                };
                let target_vector = other + offset - game.objects[name].position;
                let target_vector =
                    target_vector / (target_vector.x.powf(2.0) + target_vector.y.powf(2.0)).sqrt();
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
                        game.objects[name].position.x,
                        other.x + offset.x,
                        velocity.x,
                    ),
                    move_to(
                        game.objects[name].position.y,
                        other.y + offset.y,
                        velocity.y,
                    ),
                )
            };

            if let TargetType::StopWhenReached = target_type {
                let other = match &target {
                    Target::Object { name: other_name } => {
                        game.objects.get_obj(other_name)?.position
                    }
                    Target::Mouse => mouse.position,
                };
                let close_enough =
                    |pos: f32, other: f32, offset: f32| (pos - (other + offset)).abs() < 0.5;
                if close_enough(game.objects[name].position.x, other.x, offset.x)
                    && close_enough(game.objects[name].position.y, other.y, offset.y)
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
            game.objects[name].position += velocity;
            velocity += acceleration;
            ActiveMotion::Accelerate {
                velocity,
                acceleration,
            }
        }
        ActiveMotion::SlowDown {
            mut velocity,
            deceleration,
        } => {
            if velocity.magnitude() <= deceleration.magnitude() {
                ActiveMotion::Stop
            } else {
                game.objects[name].position += velocity;
                velocity += deceleration;
                ActiveMotion::SlowDown {
                    velocity,
                    deceleration,
                }
            }
        }
        ActiveMotion::Stop => ActiveMotion::Stop,
    };

    Ok(active_motion)
}

use wee_common::{Colour, Flip, Size, Vec2, WeeResult, AABB, PROJECTION_HEIGHT, PROJECTION_WIDTH};

use c2::prelude::*;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    default::Default,
    path::Path,
    str,
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum FrameCount {
    Frames(u32),
    Infinite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum When {
    Start,
    End,
    Exact { time: u32 },
    Random { start: u32, end: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CollisionWith {
    Object { name: String },
    Area(AABB),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MouseInteraction {
    Button { state: ButtonState },
    Hover,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    JustWon,
    JustLost,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PropertyCheck {
    Switch(SwitchState),
    Sprite(Sprite),
    FinishedAnimation,
    Timer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Trigger {
    Time(When),
    Collision(CollisionWith),
    Input(Input),
    WinStatus(WinStatus),
    Random { chance: f32 },
    CheckProperty { name: String, check: PropertyCheck },
    DifficultyLevel { levels: HashSet<u32> },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Effect {
    Freeze,
    None,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Angle {
    Current,
    Degrees(f32),
    Random { min: f32, max: f32 },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MovementDirection {
    Angle(Angle),
    Direction {
        possible_directions: HashSet<CompassDirection>,
    },
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Speed {
    VerySlow,
    Slow,
    Normal,
    Fast,
    VeryFast,
    Value(f32),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelativeTo {
    CurrentPosition,
    CurrentAngle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Acceleration {
    Continuous {
        direction: MovementDirection,
        speed: Speed,
    },
    SlowDown {
        speed: Speed,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    Accelerate(Acceleration),
    Stop,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnimationType {
    Loop,
    PlayOnce,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AngleSetter {
    Value(f32),
    Increase(f32),
    Decrease(f32),
    Match { name: String },
    Clamp { min: f32, max: f32 },
    RotateToObject { name: String },
    RotateToMouse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SizeDifference {
    Value(Size),
    Percent(Size),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FlipSetter {
    Flip,
    SetFlip(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LayerSetter {
    Value(u8),
    Increase,
    Decrease,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum JustifyText {
    Centre,
    Left,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        justify: JustifyText,
    },
    Random {
        random_actions: Vec<Action>,
    },
    EndEarly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Instruction {
    pub triggers: Vec<Trigger>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontLoadInfo {
    pub filename: String,
    pub size: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Sprite {
    Image { name: String },
    Colour(Colour),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    SlowDown {
        velocity: Vec2,
        deceleration: Vec2,
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
        frames_in_bounce: f32,
    },
}

#[derive(Clone, Debug)]
pub struct Object {
    pub sprite: Sprite,
    pub position: Vec2,
    pub size: Size,
    pub angle: f32,
    pub origin: Option<Vec2>,
    pub collision_area: Option<AABB>,
    pub flip: Flip,
    pub layer: u8,
    pub instructions: Vec<Instruction>,
    pub queued_motion: Vec<Motion>,
    pub active_motion: ActiveMotion,
    pub switch: SwitchState,
    pub timer: Option<u32>,
    pub animation: AnimationStatus,
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
pub struct GameStatus {
    pub current: WinStatus,
    pub next_frame: WinStatus,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Mouse {
    pub position: Vec2,
    pub state: ButtonState,
}

pub type Objects = IndexMap<String, Object>;

#[derive(Debug, Copy, Clone)]
pub struct FrameInfo {
    pub total: FrameCount,
    pub ran: u32,
    pub steps_taken: u32,
    pub to_run: u32,
    pub total_time_elapsed: f64,
    pub previous_frame_time: f64,
}

pub struct Game {
    pub objects: Objects,
    pub background: Vec<BackgroundPart>,
    pub frames: FrameInfo,
    pub status: GameStatus,
    pub intro_text: String,
    effect: Effect,
    pub difficulty: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameData {
    pub format_version: String,
    pub published: bool,
    pub game_type: GameType,
    pub objects: Vec<SerialiseObject>,
    pub background: Vec<BackgroundPart>,
    pub asset_files: AssetFiles,
    pub length: Length,
    pub intro_text: Option<String>,
    pub attribution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SerialiseMusic {
    pub filename: String,
    pub looped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetFiles {
    pub images: HashMap<String, String>,
    pub audio: HashMap<String, String>,
    pub music: Option<SerialiseMusic>,
    pub fonts: HashMap<String, FontLoadInfo>,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum GameType {
    Minigame,
    BossGame,
    Other,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub enum Length {
    Seconds(f32),
    Infinite,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

// For editor

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

impl Default for SerialiseObject {
    fn default() -> SerialiseObject {
        SerialiseObject {
            name: "".to_string(),
            sprite: Sprite::Colour(Colour::black()),
            position: Vec2::new(800.0, 450.0),
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

    pub fn origin_in_world(&self) -> Vec2 {
        let top_left = self.position - Vec2::new(self.half_width(), self.half_height());
        top_left + self.origin()
    }

    fn aabb(&self) -> AABB {
        AABB::new(
            self.position.x - self.half_width(),
            self.position.y - self.half_height(),
            self.position.x + self.half_width(),
            self.position.y + self.half_height(),
        )
    }

    pub fn rect(&self) -> wee_common::Rect {
        wee_common::Rect::new(
            self.position.x,
            self.position.y,
            self.size.width,
            self.size.height,
        )
    }

    pub fn full_poly(&self) -> c2::Poly {
        let aabb = self.aabb();
        let origin = self.origin_in_world();
        let aabb = aabb.move_position(-origin);
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
            *point = c2v(
                point.x() * c - point.y() * s + origin.x,
                point.x() * s + point.y() * c + origin.y,
            );
        }
        c2::Poly::from_slice(&points)
    }
}

impl Default for GameData {
    fn default() -> GameData {
        GameData {
            format_version: "0.4".to_string(),
            published: false,
            game_type: GameType::Minigame,
            objects: Vec::new(),
            background: Vec::new(),
            asset_files: AssetFiles::default(),
            length: Length::Seconds(4.0),
            intro_text: None,
            attribution: "".to_string(),
        }
    }
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

impl Object {
    pub fn rect(&self) -> wee_common::Rect {
        wee_common::Rect::new(
            self.position.x,
            self.position.y,
            self.size.width,
            self.size.height,
        )
    }
}

use std::fmt;

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
                (*start as f32) / 60.0,
                (*end as f32) / 60.0,
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
                WinStatus::JustWon => write!(f, "When you win the game"),
                WinStatus::JustLost => write!(f, "When you lose the game"),
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
                PropertyCheck::Timer => write!(f, "When {}'s timer hits zero", name),
            },
            Trigger::Random { chance } => write!(f, "With a {}% chance", chance * 100.0),
            Trigger::DifficultyLevel { levels } => {
                if levels.is_empty() {
                    write!(f, "If the difficulty is any difficulty")
                } else {
                    let mut levels: Vec<String> =
                        levels.iter().map(|level| level.to_string()).collect();
                    levels.sort();
                    let levels = levels.join(" or ");
                    write!(f, "If the difficulty is {}", levels)
                }
            }
        }
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
                    "Bounce {} between {}, {} and {}, {}",
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
            Motion::Accelerate(Acceleration::Continuous { direction, speed }) => match direction {
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
            Motion::Accelerate(Acceleration::SlowDown { speed }) => {
                write!(f, "Slow down {}", speed)
            }
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Motion(motion) => write!(f, "{}", motion),
            Action::Win => write!(f, "Win the game"),
            Action::Lose => write!(f, "Lose the game"),
            Action::Effect(effect) => match effect {
                Effect::Freeze => write!(f, "Freeze the screen"),
                Effect::None => write!(f, "No effect"),
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
                resize: _resize,
                justify,
            } => {
                let change_size = "";
                let colour = format!(
                    "red: {}, green: {}, blue: {}, alpha: {}",
                    colour.r, colour.g, colour.b, colour.a
                );
                let justification = match justify {
                    JustifyText::Left => "left",
                    JustifyText::Centre => "centre",
                };
                write!(
                    f,
                    "Draws `{}` using the {} font with colour ({}) {} justifying to the {}",
                    text, font, colour, change_size, justification
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
                AngleSetter::RotateToObject { name } => write!(f, "Rotate towards {}", name),
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
                write!(f, "Set the timer to {:.2} seconds ", (*time as f32) / 60.0)
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
            Action::Random { .. } => write!(f, "Choose a random action"),
            Action::EndEarly => write!(f, "End the game"),
        }
    }
}

use std::ops::Not;

impl Not for TextResize {
    type Output = TextResize;

    fn not(self) -> Self::Output {
        match self {
            TextResize::MatchText => TextResize::MatchObject,
            TextResize::MatchObject => TextResize::MatchText,
        }
    }
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

impl Not for Switch {
    type Output = Switch;

    fn not(self) -> Self::Output {
        match self {
            Switch::On => Switch::Off,
            Switch::Off => Switch::On,
        }
    }
}
