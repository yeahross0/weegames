#[macro_use]
extern crate imgui;

use imgui::{ImStr, ImString};
use nfd::Response;
use sdlglue::Texture;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    str,
};
use wee::*;
use wee_common::{Colour, Size, Vec2, WeeResult, AABB};

pub const SMALL_BUTTON: [f32; 2] = [100.0, 50.0];
pub const NORMAL_BUTTON: [f32; 2] = [200.0, 50.0];

trait ImguiDisplayTrigger {
    fn display(&self, ui: &imgui::Ui, sprites: &HashMap<&str, &Sprite>, images: &Images);
}

impl ImguiDisplayTrigger for Trigger {
    fn display(&self, ui: &imgui::Ui, sprites: &HashMap<&str, &Sprite>, images: &Images) {
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
                ui.tooltip(|| match sprites.get(object_name) {
                    Some(sprite) => match &sprite {
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
                    None => {
                        ui.text_colored(
                            [1.0, 0.0, 0.0, 1.0],
                            format!("Warning: Object `{}` not found", object_name),
                        );
                    }
                });
            }
        };

        let object_button_with_label = |label: &ImStr, object_name: &str| {
            if sprites.keys().any(|name| *name == object_name) {
                ui.text(label);
            } else {
                ui.text_colored([1.0, 0.0, 0.0, 1.0], label);
            }
            object_tooltip(object_name);
        };

        let object_button = |object_name: &str| {
            object_button_with_label(&ImString::from(object_name.to_string()), object_name);
        };

        let image_button_with_label = |label: &ImStr, image_name: &str| {
            ui.text(label);
            image_tooltip(image_name);
        };

        let image_button = |image_name: &str| {
            image_button_with_label(&ImString::from(image_name.to_string()), image_name);
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
            Trigger::Collision(CollisionWith::Object { name }) => {
                ui.text("While this object collides with");
                same_line();
                object_button(name);
            }
            Trigger::Input(Input::Mouse { over, interaction }) => {
                if let MouseOver::Anywhere = over {
                    ui.text(self.to_string());
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
                _ => ui.text(self.to_string()),
            },
            _ => ui.text(self.to_string()),
        };
    }
}

trait ImguiDisplayAction {
    fn display(&self, ui: &imgui::Ui, indent: usize);
}

impl ImguiDisplayAction for Action {
    fn display(&self, ui: &imgui::Ui, indent: usize) {
        ui.text("\t".repeat(indent));
        ui.same_line_with_spacing(0.0, 0.0);

        match self {
            Action::Random { random_actions } => {
                ui.text("Chooses a random action");
                for action in random_actions {
                    action.display(ui, indent + 1);
                }
            }
            _ => ui.text(self.to_string()),
        };
    }
}

pub fn right_window(
    ui: &imgui::Ui,
    object: &mut SerialiseObject,
    object_names: &Vec<&str>,
    sprites: &HashMap<&str, &Sprite>,
    asset_files: &mut AssetFiles,
    game_length: f32,
    images: &mut Images,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
    instruction_index: &mut usize,
    instruction_focus: &mut InstructionFocus,
) {
    imgui::ChildWindow::new(im_str!("Right"))
        .size([0.0, 0.0])
        .scroll_bar(false)
        .build(ui, || {
            fn tab_bar<F: FnOnce()>(label: &ImStr, f: F) {
                unsafe {
                    if imgui::sys::igBeginTabBar(label.as_ptr(), 0) {
                        f();
                        imgui::sys::igEndTabItem();
                    }
                }
            }

            fn tab_item<F: FnOnce()>(label: &ImStr, f: F) {
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
            tab_bar(im_str!("Tab Bar"), || {
                tab_item(im_str!("Properties"), || {
                    properties_window(ui, object, asset_files, images, filename);
                });
                tab_item(im_str!("Instructions"), || {
                    object.instructions.choose(
                        ui,
                        object_names,
                        sprites,
                        asset_files,
                        game_length,
                        images,
                        filename,
                        instruction_mode,
                        instruction_index,
                        instruction_focus,
                    );
                });
            });
        });
}

pub enum InstructionMode {
    View,
    Edit,
    AddTrigger,
    AddAction,
    EditTrigger,
    EditAction,
}

#[derive(Debug, PartialEq)]
pub enum InstructionFocus {
    Trigger { index: usize },
    Action { index: usize },
    None,
}

trait ChooseInstruction {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        instruction_mode: &mut InstructionMode,
        focus: &mut InstructionFocus,
    );
}

impl ChooseInstruction for Instruction {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        instruction_mode: &mut InstructionMode,
        focus: &mut InstructionFocus,
    ) {
        ui.text("Triggers:");
        for (i, trigger) in self.triggers.iter().enumerate() {
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
        for (i, action) in self.actions.iter().enumerate() {
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
                        self.triggers.swap(*selected_index, *selected_index - 1);
                        *selected_index -= 1;
                    }
                }
                InstructionFocus::Action {
                    index: selected_index,
                } => {
                    if *selected_index > 0 {
                        self.actions.swap(*selected_index, *selected_index - 1);
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
                    if *selected_index + 1 < self.triggers.len() {
                        self.triggers.swap(*selected_index, *selected_index + 1);
                        *selected_index += 1;
                    }
                }
                InstructionFocus::Action {
                    index: selected_index,
                } => {
                    if *selected_index + 1 < self.actions.len() {
                        self.actions.swap(*selected_index, *selected_index + 1);
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
                    if !self.triggers.is_empty() {
                        self.triggers.remove(*selected_index);
                        if *selected_index > 0 {
                            *selected_index -= 1;
                        }
                    }
                }
                InstructionFocus::Action {
                    index: selected_index,
                } => {
                    if !self.actions.is_empty() {
                        self.actions.remove(*selected_index);
                        if *selected_index > 0 {
                            *selected_index -= 1;
                        }
                    }
                }
                InstructionFocus::None => {}
            }
        }
    }
}

trait Choose {
    fn choose(&mut self, ui: &imgui::Ui);
}

trait ChooseWhen {
    fn choose(&mut self, ui: &imgui::Ui, game_length: f32);
}

impl ChooseWhen for When {
    fn choose(&mut self, ui: &imgui::Ui, game_length: f32) {
        let mut current_when_position = match self {
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
            *self = match current_when_position {
                0 => When::Start,
                1 => When::End,
                2 => When::Exact { time: 0 },
                3 => When::Random { start: 0, end: 60 },
                _ => unreachable!(),
            };
        }

        match self {
            When::Exact { time } => {
                let mut changed_time = *time as i32;
                ui.input_int(im_str!("Time"), &mut changed_time).build();
                *time = changed_time as u32;
            }
            When::Random { start, end } => {
                let mut frames = [*start as i32, *end as i32];
                ui.drag_int2(im_str!("Time"), &mut frames)
                    .min(0)
                    .max((game_length * 60.0) as i32)
                    .build();
                *start = frames[0] as u32;
                *end = frames[1] as u32;
            }
            _ => {}
        }
    }
}

trait ChooseCollisionWith {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseCollisionWith for CollisionWith {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let mut collision_type = if let CollisionWith::Object { .. } = self {
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
            *self = if collision_type == 0 {
                CollisionWith::Object {
                    name: object_names[0].to_string(),
                }
            } else {
                CollisionWith::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))
            };
        }

        match self {
            CollisionWith::Object { name } => {
                let mut current_object = object_names
                    .iter()
                    .position(|obj_name| obj_name == name)
                    .unwrap();
                let keys: Vec<ImString> = object_names
                    .iter()
                    .map(|name| ImString::from(name.to_string()))
                    .collect();
                let combo_keys: Vec<_> = keys.iter().collect();
                if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                    &ui,
                    &mut current_object,
                    &combo_keys,
                ) {
                    *name = object_names[current_object].to_string();
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
}

trait ChooseMouseOver {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseMouseOver for MouseOver {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let mut input_type = match self {
            MouseOver::Object { .. } => 0,
            MouseOver::Area(_) => 1,
            MouseOver::Anywhere => 2,
        };
        let input_typename = if input_type == 0 {
            "Object".to_string()
        } else {
            "Area".to_string()
        };
        if imgui::Slider::new(im_str!("Mouse Over"), std::ops::RangeInclusive::new(0, 1))
            .display_format(&ImString::from(input_typename))
            .build(&ui, &mut input_type)
        {
            *self = if input_type == 0 {
                MouseOver::Object {
                    name: object_names[0].to_string(),
                }
            } else if input_type == 1 {
                MouseOver::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))
            } else {
                MouseOver::Anywhere
            };
        }
        match self {
            MouseOver::Object { name } => {
                let mut current_object = object_names
                    .iter()
                    .position(|obj_name| obj_name == name)
                    .unwrap();
                let keys: Vec<ImString> = object_names
                    .iter()
                    .map(|name| ImString::from(name.to_string()))
                    .collect();
                let combo_keys: Vec<_> = keys.iter().collect();
                if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                    &ui,
                    &mut current_object,
                    &combo_keys,
                ) {
                    *name = object_names[current_object].to_string();
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
    }
}

impl Choose for MouseInteraction {
    fn choose(&mut self, ui: &imgui::Ui) {
        let button_states = [
            im_str!("Button Up"),
            im_str!("Button Down"),
            im_str!("Button Press"),
            im_str!("Button Release"),
            im_str!("Hover"),
        ];
        let mut current_button_state = match self {
            MouseInteraction::Button { state } => *state as usize,
            MouseInteraction::Hover => 4,
        };
        if imgui::ComboBox::new(im_str!("Button State")).build_simple_string(
            &ui,
            &mut current_button_state,
            &button_states,
        ) {
            *self = match current_button_state {
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
}

impl Choose for WinStatus {
    fn choose(&mut self, ui: &imgui::Ui) {
        let win_states = [
            im_str!("Won"),
            im_str!("Lost"),
            im_str!("Has Been Won"),
            im_str!("Has Been Lost"),
            im_str!("Not Yet Won"),
            im_str!("Not Yet Lost"),
        ];
        let mut current_status = *self as usize;

        if imgui::ComboBox::new(im_str!("Win State")).build_simple_string(
            &ui,
            &mut current_status,
            &win_states,
        ) {
            *self = match current_status {
                0 => WinStatus::Won,
                1 => WinStatus::Lost,
                2 => WinStatus::HasBeenWon,
                3 => WinStatus::HasBeenLost,
                4 => WinStatus::NotYetWon,
                5 => WinStatus::NotYetLost,
                _ => unreachable!(),
            };
        }
    }
}

trait ChoosePercent {
    fn choose_percent(&mut self, ui: &imgui::Ui);
}

impl ChoosePercent for f32 {
    fn choose_percent(&mut self, ui: &imgui::Ui) {
        let mut chance_percent = *self * 100.0;
        ui.drag_float(im_str!("Chance"), &mut chance_percent)
            .min(0.0)
            .max(100.0)
            .speed(1.0)
            .display_format(im_str!("%.01f%%"))
            .build();
        *self = chance_percent / 100.0;
    }
}

trait ChooseObject {
    fn choose_object(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseObject for String {
    fn choose_object(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let mut current_object = object_names
            .iter()
            .position(|obj_name| obj_name == self)
            .unwrap();
        let keys: Vec<ImString> = object_names
            .iter()
            .map(|name| ImString::from(name.to_string()))
            .collect();
        let combo_keys: Vec<_> = keys.iter().collect();
        if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
            &ui,
            &mut current_object,
            &combo_keys,
        ) {
            *self = object_names[current_object].to_string();
        }
    }
}

impl Choose for SwitchState {
    fn choose(&mut self, ui: &imgui::Ui) {
        let switch_states = [
            im_str!("On"),
            im_str!("Off"),
            im_str!("Switched On"),
            im_str!("Switched Off"),
        ];

        let mut current_switch_state = *self as usize;

        if imgui::ComboBox::new(im_str!("Switch State")).build_simple_string(
            &ui,
            &mut current_switch_state,
            &switch_states,
        ) {
            *self = match current_switch_state {
                0 => SwitchState::On,
                1 => SwitchState::Off,
                2 => SwitchState::SwitchedOn,
                3 => SwitchState::SwitchedOff,
                _ => SwitchState::On,
            };
        }
    }
}

trait SelectSpriteType {
    fn select_type(&mut self, ui: &imgui::Ui, images: &Images);
}

impl SelectSpriteType for Sprite {
    fn select_type(&mut self, ui: &imgui::Ui, images: &Images) {
        let mut sprite_type = if let Sprite::Image { .. } = self {
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
            *self = if sprite_type == 0 {
                Sprite::Image {
                    name: images.keys().next().unwrap().clone(),
                }
            } else {
                Sprite::Colour(Colour::black())
            };
        }
    }
}
trait ChooseNewImage {
    fn choose_new_image<P: AsRef<Path>>(
        &mut self,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        path: P,
    );
}

impl ChooseNewImage for Sprite {
    fn choose_new_image<P: AsRef<Path>>(
        &mut self,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        path: P,
    ) {
        let first_key = choose_image_from_files(asset_files, images, path);
        match first_key {
            Some(key) => {
                *self = Sprite::Image { name: key.clone() };
            }
            None => {
                log::error!("None of the new images loaded correctly");
            }
        }
    }
}

trait ChooseSprite {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
    );
}

impl ChooseSprite for Sprite {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
    ) {
        self.select_type(ui, images);

        match self {
            Sprite::Image { name: image_name } => {
                // TODO: Tidy up
                let path = match filename {
                    Some(filename) => Path::new(filename).parent().unwrap().join("images"),
                    None => Path::new("games").to_owned(),
                };

                if images.is_empty() {
                    if ui.button(im_str!("Add a New Image"), NORMAL_BUTTON) {
                        self.choose_new_image(asset_files, images, path);
                    }
                } else {
                    let mut current_image =
                        images.keys().position(|k| k == image_name).unwrap_or(0);
                    let keys = {
                        let mut keys: Vec<ImString> =
                            images.keys().map(|k| ImString::from(k.clone())).collect();

                        keys.push(ImString::new("Add a New Image"));

                        keys
                    };

                    let image_names: Vec<&ImString> = keys.iter().collect();

                    if imgui::ComboBox::new(im_str!("Image")).build_simple_string(
                        &ui,
                        &mut current_image,
                        &image_names,
                    ) {
                        if current_image == image_names.len() - 1 {
                            self.choose_new_image(asset_files, images, path);
                        } else {
                            match images.keys().nth(current_image) {
                                Some(image) => {
                                    *self = Sprite::Image {
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
    }
}

trait ChoosePropertyCheck {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        images: &mut Images,
        asset_files: &mut AssetFiles,
        filename: &Option<String>,
    );
}

impl ChoosePropertyCheck for PropertyCheck {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        images: &mut Images,
        asset_files: &mut AssetFiles,
        filename: &Option<String>,
    ) {
        match self {
            PropertyCheck::Switch(switch_state) => {
                switch_state.choose(ui);
            }
            PropertyCheck::Sprite(sprite) => {
                sprite.choose(ui, asset_files, images, filename);
            }
            _ => {}
        }
    }
}

trait ChooseTrigger {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        game_length: f32,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
    );
}

impl ChooseTrigger for Trigger {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        game_length: f32,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
    ) {
        let first_name = || object_names[0].to_string();
        let mut current_trigger_position = match self {
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
            *self = match current_trigger_position {
                0 => Trigger::Time(When::Start),
                1 => Trigger::Collision(CollisionWith::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))),
                2 => Trigger::Input(Input::Mouse {
                    over: MouseOver::Anywhere,
                    interaction: MouseInteraction::Hover,
                }),
                3 => Trigger::WinStatus(WinStatus::Won),
                4 => Trigger::Random { chance: 0.5 },
                5 => Trigger::CheckProperty {
                    name: first_name(),
                    check: PropertyCheck::Switch(SwitchState::On),
                },
                6 => Trigger::CheckProperty {
                    name: first_name(),
                    check: PropertyCheck::Sprite(Sprite::Colour(Colour::black())),
                },
                7 => Trigger::CheckProperty {
                    name: first_name(),
                    check: PropertyCheck::FinishedAnimation,
                },
                8 => Trigger::CheckProperty {
                    name: first_name(),
                    check: PropertyCheck::Timer,
                },
                _ => unreachable!(),
            }
        }
        match self {
            Trigger::Time(when) => {
                when.choose(ui, game_length);
            }
            Trigger::Collision(with) => {
                with.choose(ui, object_names);
            }
            Trigger::Input(Input::Mouse { over, interaction }) => {
                over.choose(ui, object_names);
                interaction.choose(ui);
            }
            Trigger::WinStatus(status) => {
                status.choose(ui);
            }
            Trigger::Random { chance } => {
                chance.choose_percent(ui);
            }
            Trigger::CheckProperty { name, check } => {
                name.choose_object(ui, object_names);
                check.choose(ui, images, asset_files, filename);
            }
        }
        if ui.small_button(im_str!("Back")) {
            *instruction_mode = InstructionMode::Edit;
        }
    }
}

trait ChooseAction {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
    );
}

impl ChooseAction for Action {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        asset_files: &mut AssetFiles,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
    ) {
        let first_name = || object_names[0].to_string();
        let mut current_action_position = match self {
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
            *self = match current_action_position {
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
                    speed: Speed::Normal,
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

        match self {
            Action::Effect(effect) => {
                effect.choose(ui);
            }
            Action::Motion(motion) => {
                motion.choose(ui, object_names);
            }
            Action::PlaySound { name } => {
                // TODO: Pass in assets
            }
            Action::SetProperty(setter) => {
                setter.choose(ui, object_names, images, asset_files, filename);
            }
            Action::Animate {
                animation_type,
                sprites,
                speed,
            } => {
                animation_type.choose(ui);

                // TODO: Add ability to add sprites here

                speed.choose(ui);
            }
            Action::DrawText {
                text,
                font,
                colour,
                resize,
            } => {
                text.choose(ui);
                // TODO: Set font
                colour.choose(ui);
                resize.choose(ui);
            }
            Action::Random { random_actions } => {
                // TODO: Finish this
                ui.text("Actions:");
                fn list_actions(ui: &imgui::Ui, actions: &[Action]) {
                    for action in actions.iter() {
                        ui.text(action.to_string());
                    }
                }
                list_actions(&ui, &random_actions);
                if ui.button(im_str!("Add Action"), SMALL_BUTTON) {
                    random_actions.push(Action::Win);
                }
            }
            _ => {}
        }

        if ui.small_button(im_str!("Back")) {
            *instruction_mode = InstructionMode::Edit;
        }
    }
}

impl Choose for TextResize {
    fn choose(&mut self, ui: &imgui::Ui) {
        if ui.radio_button_bool(
            im_str!("Adjust Object Size to Match Text"),
            *self == TextResize::MatchText,
        ) {
            *self = !*self;
        }
    }
}

impl Choose for Colour {
    fn choose(&mut self, ui: &imgui::Ui) {
        let mut colour_array = [self.r, self.g, self.b, self.a];
        imgui::ColorEdit::new(im_str!("Colour"), &mut colour_array)
            .alpha(true)
            .build(ui);
        *self = Colour::rgba(
            colour_array[0],
            colour_array[1],
            colour_array[2],
            colour_array[3],
        );
    }
}

impl Choose for String {
    fn choose(&mut self, ui: &imgui::Ui) {
        let mut change_text = ImString::from(self.to_owned());
        ui.input_text(im_str!("Text"), &mut change_text)
            .resize_buffer(true)
            .build();
        *self = change_text.to_str().to_owned();
    }
}

impl Choose for AnimationType {
    fn choose(&mut self, ui: &imgui::Ui) {
        if ui.radio_button_bool(im_str!("Loop Animation"), *self == AnimationType::Loop) {
            *self = !*self;
        }
    }
}

trait ChoosePropertySetter {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        images: &mut Images,
        asset_files: &mut AssetFiles,
        filename: &Option<String>,
    );
}

impl ChoosePropertySetter for PropertySetter {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        images: &mut Images,
        asset_files: &mut AssetFiles,
        filename: &Option<String>,
    ) {
        let property_types = [
            im_str!("Sprite"),
            im_str!("Angle"),
            im_str!("Size"),
            im_str!("Switch"),
            im_str!("Timer"),
            im_str!("Flip Horizontal"),
            im_str!("Flip Vertical"),
            im_str!("Layer"),
        ];
        let mut current_property_position = match self {
            PropertySetter::Sprite(_) => 0,
            PropertySetter::Angle(_) => 1,
            PropertySetter::Size(_) => 2,
            PropertySetter::Switch(_) => 3,
            PropertySetter::Timer { .. } => 4,
            PropertySetter::FlipHorizontal(_) => 5,
            PropertySetter::FlipVertical(_) => 6,
            PropertySetter::Layer(_) => 7,
        };
        if imgui::ComboBox::new(im_str!("Property Type")).build_simple_string(
            &ui,
            &mut current_property_position,
            &property_types,
        ) {
            *self = match current_property_position {
                0 => PropertySetter::Sprite(Sprite::Colour(Colour::black())),
                1 => PropertySetter::Angle(AngleSetter::Value(0.0)),
                2 => PropertySetter::Size(SizeSetter::Value(Size::new(1.0, 1.0))),
                3 => PropertySetter::Switch(Switch::On),
                4 => PropertySetter::Timer { time: 60 },
                5 => PropertySetter::FlipHorizontal(FlipSetter::Flip),
                6 => PropertySetter::FlipVertical(FlipSetter::Flip),
                7 => PropertySetter::Layer(LayerSetter::Value(0)),
                _ => unreachable!(),
            };
        }
        match self {
            PropertySetter::Sprite(sprite) => {
                sprite.choose(ui, asset_files, images, filename);
            }
            PropertySetter::Angle(angle_setter) => {
                angle_setter.choose(ui, object_names);
            }
            PropertySetter::Size(size_setter) => {
                size_setter.choose(ui);
            }
            PropertySetter::Switch(switch) => {
                switch.choose(ui);
            }
            PropertySetter::Timer { time } => {
                time.choose(ui, im_str!("Time"));
            }
            PropertySetter::FlipHorizontal(flip_setter)
            | PropertySetter::FlipVertical(flip_setter) => {
                flip_setter.choose(ui);
            }
            PropertySetter::Layer(layer_setter) => {
                layer_setter.choose(ui);
            }
        }
    }
}

impl Choose for FlipSetter {
    fn choose(&mut self, ui: &imgui::Ui) {
        self.radio(ui);

        if let FlipSetter::SetFlip(flip) = self {
            if ui.radio_button_bool(im_str!("Switch"), *flip) {
                *flip = !*flip;
            }
        }
    }
}

impl Choose for LayerSetter {
    fn choose(&mut self, ui: &imgui::Ui) {
        self.radio(ui);

        if let LayerSetter::Value(value) = self {
            let mut v = *value as i32;
            ui.input_int(im_str!("Layer"), &mut v).build();
            *value = v as u8;
        }
    }
}

trait ChooseWithLabel {
    fn choose(&mut self, ui: &imgui::Ui, label: &ImStr);
}

impl ChooseWithLabel for u32 {
    fn choose(&mut self, ui: &imgui::Ui, label: &ImStr) {
        let mut value = *self as i32;
        ui.input_int(label, &mut value).build();
        *self = value as u32;
    }
}

impl Choose for Switch {
    fn choose(&mut self, ui: &imgui::Ui) {
        if ui.radio_button_bool(im_str!("Switch"), *self == Switch::On) {
            *self = !*self;
        }
    }
}

impl Choose for SizeSetter {
    fn choose(&mut self, ui: &imgui::Ui) {
        self.combo(ui);
        match self {
            SizeSetter::Value(size) => {
                ui.input_float(im_str!("Width"), &mut size.width).build();
                ui.input_float(im_str!("Height"), &mut size.height).build();
            }
            SizeSetter::Grow(size_difference) | SizeSetter::Shrink(size_difference) => {
                size_difference.radio(ui);

                match size_difference {
                    SizeDifference::Value(size) | SizeDifference::Percent(size) => {
                        ui.input_float(im_str!("Width"), &mut size.width).build();
                        ui.input_float(im_str!("Height"), &mut size.height).build();
                    }
                }
            }
            SizeSetter::Clamp { min, max } => {
                ui.input_float(im_str!("Min Width"), &mut min.width).build();
                ui.input_float(im_str!("Min Height"), &mut min.height)
                    .build();
                ui.input_float(im_str!("Max Width"), &mut max.width).build();
                ui.input_float(im_str!("Max Height"), &mut max.height)
                    .build();
            }
        }
    }
}

impl ChooseWithObjectNames for AngleSetter {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let angle_types = [
            im_str!("Value"),
            im_str!("Increase"),
            im_str!("Decrease"),
            im_str!("Match"),
            im_str!("Clamp"),
            im_str!("Rotate To Mouse"),
        ];
        let mut current_angle_position = match self {
            AngleSetter::Value(_) => 0,
            AngleSetter::Increase(_) => 1,
            AngleSetter::Decrease(_) => 2,
            AngleSetter::Match { .. } => 3,
            AngleSetter::Clamp { .. } => 4,
            AngleSetter::RotateToMouse => 5,
        };
        if imgui::ComboBox::new(im_str!("Angle Setter")).build_simple_string(
            &ui,
            &mut current_angle_position,
            &angle_types,
        ) {
            *self = match current_angle_position {
                0 => AngleSetter::Value(0.0),
                1 => AngleSetter::Increase(0.0),
                2 => AngleSetter::Decrease(0.0),
                3 => AngleSetter::Match {
                    name: object_names[0].to_string(),
                },
                4 => AngleSetter::Clamp {
                    min: 0.0,
                    max: 360.0,
                },
                5 => AngleSetter::RotateToMouse,
                _ => unreachable!(),
            };
        }

        match self {
            AngleSetter::Value(value) => {
                value.choose_angle(ui);
            }
            AngleSetter::Increase(value) => {
                ui.input_float(im_str!("Angle"), value).build();
            }
            AngleSetter::Decrease(value) => {
                ui.input_float(im_str!("Angle"), value).build();
            }
            AngleSetter::Match { name } => {
                name.choose_object(ui, object_names);
            }
            AngleSetter::Clamp { min, max } => {
                ui.input_float(im_str!("Min Angle"), min).build();
                ui.input_float(im_str!("Max Angle"), max).build();
            }
            _ => {}
        }
    }
}

impl Choose for Effect {
    fn choose(&mut self, ui: &imgui::Ui) {
        let mut effect_type = if *self == Effect::None { 0 } else { 1 };
        let effect_typename = if effect_type == 0 {
            "None".to_string()
        } else {
            "Freeze".to_string()
        };
        if imgui::Slider::new(im_str!("Effect"), std::ops::RangeInclusive::new(0, 1))
            .display_format(&ImString::from(effect_typename))
            .build(&ui, &mut effect_type)
        {
            *self = if effect_type == 0 {
                Effect::None
            } else {
                Effect::Freeze
            };
        }
    }
}

trait ChooseMotion {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseMotion for Motion {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let mut current_motion_position = match self {
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
            *self = match current_motion_position {
                0 => Motion::Stop,
                1 => Motion::GoStraight {
                    direction: MovementDirection::Angle(Angle::Current),
                    speed: Speed::Normal,
                },
                2 => Motion::JumpTo(JumpLocation::Mouse),
                3 => Motion::Roam {
                    movement_type: MovementType::Wiggle,
                    area: AABB::new(0.0, 0.0, 0.0, 0.0),
                    speed: Speed::Normal,
                },
                4 => Motion::Swap {
                    name: object_names[0].to_string(),
                },
                5 => Motion::Target {
                    target: Target::Mouse,
                    target_type: TargetType::StopWhenReached,
                    offset: Vec2::zero(),
                    speed: Speed::Normal,
                },
                6 => Motion::Accelerate {
                    direction: MovementDirection::Angle(Angle::Current),
                    speed: Speed::Normal,
                },
                _ => unreachable!(),
            };
        }

        match self {
            Motion::GoStraight { direction, speed } | Motion::Accelerate { direction, speed } => {
                direction.choose(ui);
                speed.choose(ui);
            }
            Motion::JumpTo(jump_location) => {
                jump_location.choose(ui, object_names);
            }
            Motion::Roam {
                movement_type,
                area,
                speed,
            } => {
                movement_type.choose(ui);
                area.choose(ui);
                speed.choose(ui);
            }
            Motion::Swap { name } => {
                name.choose_object(ui, object_names);
            }
            Motion::Target {
                target,
                target_type,
                offset,
                speed,
            } => {
                target.choose(ui, object_names);
                target_type.choose(ui);

                ui.input_float(im_str!("Offset X"), &mut offset.x).build();
                ui.input_float(im_str!("Offset Y"), &mut offset.y).build();

                speed.choose(ui);
            }
            _ => {}
        }
    }
}

impl Choose for MovementDirection {
    fn choose(&mut self, ui: &imgui::Ui) {
        let direction_types = [
            im_str!("Current Angle"),
            im_str!("Chosen Angle"),
            im_str!("Random Angle"),
            im_str!("Direction"),
        ];
        let mut current_direction_position = match self {
            MovementDirection::Angle(Angle::Current) => 0,
            MovementDirection::Angle(Angle::Degrees(_)) => 1,
            MovementDirection::Angle(Angle::Random { .. }) => 2,
            MovementDirection::Direction { .. } => 3,
        };
        if imgui::ComboBox::new(im_str!("Movement Direction")).build_simple_string(
            &ui,
            &mut current_direction_position,
            &direction_types,
        ) {
            *self = match current_direction_position {
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
        match self {
            MovementDirection::Angle(Angle::Degrees(angle)) => {
                angle.choose_angle(ui);
            }
            MovementDirection::Angle(Angle::Random { min, max }) => {
                ui.input_float(im_str!("Min Angle"), min).build();
                ui.input_float(im_str!("Max Angle"), max).build();
            }
            MovementDirection::Direction {
                possible_directions,
            } => {
                possible_directions.choose(ui);
            }
            _ => {}
        }
    }
}

trait ChooseAngle {
    fn choose_angle(&mut self, ui: &imgui::Ui);
}

impl ChooseAngle for f32 {
    fn choose_angle(&mut self, ui: &imgui::Ui) {
        let mut radians = self.to_radians();
        imgui::AngleSlider::new(im_str!("Angle"))
            .min_degrees(-360.0)
            .max_degrees(360.0)
            .build(ui, &mut radians);
        *self = radians.to_degrees();
    }
}

impl Choose for Speed {
    fn choose(&mut self, ui: &imgui::Ui) {
        self.combo(ui);
        if let Speed::Value(value) = self {
            ui.input_float(im_str!("Speed"), value).build();
        }
    }
}

impl Choose for HashSet<CompassDirection> {
    fn choose(&mut self, ui: &imgui::Ui) {
        fn direction_checkbox(
            ui: &imgui::Ui,
            possible_directions: &mut HashSet<CompassDirection>,
            label: &ImStr,
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
        direction_checkbox(&ui, self, im_str!("Up"), CompassDirection::Up);
        direction_checkbox(&ui, self, im_str!("Up Right"), CompassDirection::UpRight);
        direction_checkbox(&ui, self, im_str!("Right"), CompassDirection::Right);
        direction_checkbox(
            &ui,
            self,
            im_str!("Down Right"),
            CompassDirection::DownRight,
        );
        direction_checkbox(&ui, self, im_str!("Down"), CompassDirection::Down);
        direction_checkbox(&ui, self, im_str!("Down Left"), CompassDirection::DownLeft);
        direction_checkbox(&ui, self, im_str!("Left"), CompassDirection::Left);
        direction_checkbox(&ui, self, im_str!("Up Left"), CompassDirection::UpLeft);
    }
}

trait ChooseJumpLocation {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseJumpLocation for JumpLocation {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        let jump_types = [
            im_str!("Point"),
            im_str!("Area"),
            im_str!("Mouse"),
            im_str!("Object"),
            im_str!("Relative"),
            im_str!("Clamp Position"),
        ];
        let mut current_jump_position = match self {
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
            *self = match current_jump_position {
                0 => JumpLocation::Point(Vec2::zero()),
                1 => JumpLocation::Area(AABB::new(0.0, 0.0, 1600.0, 900.0)),
                2 => JumpLocation::Mouse,
                3 => JumpLocation::Object {
                    name: object_names[0].to_string(),
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

        match self {
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
                    .map(|name| ImString::from(name.to_string()))
                    .collect();
                let combo_keys: Vec<_> = keys.iter().collect();
                if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
                    &ui,
                    &mut current_object,
                    &combo_keys,
                ) {
                    *name = object_names[current_object].to_string();
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
}

impl Choose for MovementType {
    fn choose(&mut self, ui: &imgui::Ui) {
        let movement_types = [
            im_str!("Wiggle"),
            im_str!("Insect"),
            im_str!("Reflect"),
            im_str!("Bounce"),
        ];
        let mut current_movement_position = match self {
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
            *self = match current_movement_position {
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

        match self {
            MovementType::Reflect {
                initial_direction,
                movement_handling,
            } => {
                let movement_handling_types = [im_str!("Anywhere"), im_str!("Try Not To Overlap")];

                let mut current_movement = *movement_handling as usize;

                if imgui::ComboBox::new(im_str!("Movement Handling")).build_simple_string(
                    &ui,
                    &mut current_movement,
                    &movement_handling_types,
                ) {
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
                            label: &ImStr,
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
                if ui.radio_button_bool(im_str!("No Initial Direction"), *initial_direction == None)
                {
                    *initial_direction = None;
                }
            }
            _ => {}
        }
    }
}

impl Choose for AABB {
    fn choose(&mut self, ui: &imgui::Ui) {
        ui.input_float(im_str!("Area Min X"), &mut self.min.x)
            .build();
        ui.input_float(im_str!("Area Min Y"), &mut self.min.y)
            .build();
        ui.input_float(im_str!("Area Max X"), &mut self.max.x)
            .build();
        ui.input_float(im_str!("Area Max Y"), &mut self.max.y)
            .build();
    }
}

trait ChooseWithObjectNames {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>);
}

impl ChooseWithObjectNames for Target {
    fn choose(&mut self, ui: &imgui::Ui, object_names: &Vec<&str>) {
        if ui.radio_button_bool(im_str!("Target Object"), *self != Target::Mouse) {
            *self = Target::Object {
                name: object_names[0].to_string(),
            };
        }
        if ui.radio_button_bool(im_str!("Target Mouse"), *self == Target::Mouse) {
            *self = Target::Mouse;
        }
        if let Target::Object { name } = self {
            name.choose_object(ui, object_names);
        }
    }
}

impl Choose for TargetType {
    fn choose(&mut self, ui: &imgui::Ui) {
        if ui.radio_button_bool(im_str!("Follow Object"), *self == TargetType::Follow) {
            *self = TargetType::Follow;
        }
        if ui.radio_button_bool(
            im_str!("Stop When Reached"),
            *self == TargetType::StopWhenReached,
        ) {
            *self = TargetType::StopWhenReached;
        }
    }
}

trait ChooseInstructions {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        sprites: &HashMap<&str, &Sprite>,
        asset_files: &mut AssetFiles,
        game_length: f32,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
        instruction_index: &mut usize,
        focus: &mut InstructionFocus,
    );
}

impl ChooseInstructions for Vec<Instruction> {
    fn choose(
        &mut self,
        ui: &imgui::Ui,
        object_names: &Vec<&str>,
        sprites: &HashMap<&str, &Sprite>,
        asset_files: &mut AssetFiles,
        game_length: f32,
        images: &mut Images,
        filename: &Option<String>,
        instruction_mode: &mut InstructionMode,
        selected_index: &mut usize,
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
                        for (i, instruction) in self.iter_mut().enumerate() {
                            ui.tree_node(&im_str!("Instruction {}", i + 1))
                                .flags(imgui::ImGuiTreeNodeFlags::SpanAvailWidth)
                                .bullet(true)
                                .leaf(true)
                                .selected(*selected_index == i)
                                .build(|| {
                                    if ui.is_item_clicked(imgui::MouseButton::Left) {
                                        *selected_index = i;
                                    }
                                    if instruction.triggers.is_empty() {
                                        ui.text("\tEvery frame")
                                    } else {
                                        for trigger in instruction.triggers.iter() {
                                            trigger.display(ui, sprites, images);
                                        }
                                    }

                                    ui.text("\t\tThen:");

                                    if instruction.actions.is_empty() {
                                        ui.text("\tDo nothing")
                                    } else {
                                        for action in instruction.actions.iter() {
                                            action.display(ui, 0);
                                        }
                                    }
                                });
                            ui.separator();
                        }
                    });
                if ui.small_button(im_str!("Up")) && *selected_index > 0 {
                    self.swap(*selected_index, *selected_index - 1);
                    *selected_index -= 1;
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Down")) && *selected_index + 1 < self.len() {
                    self.swap(*selected_index, *selected_index + 1);
                    *selected_index += 1;
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Add")) {
                    self.push(wee::Instruction {
                        triggers: Vec::new(),
                        actions: Vec::new(),
                    });
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Edit")) {
                    *instruction_mode = InstructionMode::Edit;
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Delete")) && !self.is_empty() {
                    self.remove(*selected_index);
                    if *selected_index > 0 {
                        *selected_index -= 1;
                    }
                }
            }
            InstructionMode::Edit => {
                self[*selected_index].choose(ui, instruction_mode, focus);
            }
            InstructionMode::AddTrigger => {
                let instruction = &mut self[*selected_index];
                instruction.triggers.push(Trigger::Time(When::Start));
                *focus = InstructionFocus::Trigger {
                    index: instruction.triggers.len() - 1,
                };
                *instruction_mode = InstructionMode::EditTrigger;
            }
            InstructionMode::AddAction => {
                let instruction = &mut self[*selected_index];
                instruction.actions.push(Action::Win);
                *focus = InstructionFocus::Action {
                    index: instruction.actions.len() - 1,
                };
                *instruction_mode = InstructionMode::EditAction;
            }
            InstructionMode::EditTrigger => {
                //let object_names: Vec<String> = game.objects.iter().map(|o| o.name.clone()).collect();
                let trigger_index = match focus {
                    InstructionFocus::Trigger { index } => *index,
                    _ => unreachable!(),
                };
                self[*selected_index].triggers[trigger_index].choose(
                    ui,
                    object_names,
                    game_length,
                    asset_files,
                    images,
                    filename,
                    instruction_mode,
                );
            }
            InstructionMode::EditAction => {
                //let object_names: Vec<String> = objects.iter().map(|o| o.name.clone()).collect();
                let action_index = match focus {
                    InstructionFocus::Action { index } => *index,
                    _ => unreachable!(),
                };
                self[*selected_index].actions[action_index].choose(
                    ui,
                    object_names,
                    asset_files,
                    images,
                    filename,
                    instruction_mode,
                );
            }
        }
    }
}

trait EnumSetters: Sized {
    fn to_value(&self) -> usize;

    fn from_value(value: usize) -> Self;

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F);

    fn combo(&mut self, ui: &imgui::Ui) {
        self.component(|s, label, types| {
            let mut position = s.to_value();
            if imgui::ComboBox::new(label).build_simple_string(&ui, &mut position, types) {
                *s = Self::from_value(position);
            }
        })
    }

    fn radio(&mut self, ui: &imgui::Ui) {
        self.component(|s, _, types| {
            for (i, t) in types.iter().enumerate() {
                if ui.radio_button_bool(t, s.to_value() == i) {
                    *s = Self::from_value(i);
                }
            }
        })
    }

    fn radio_bool(&mut self, label: &ImStr, ui: &imgui::Ui) {
        if ui.radio_button_bool(im_str!("Loop Animation"), self.to_value() == 0) {
            *self = Self::from_value(!self.to_value());
        }
    }
}

impl EnumSetters for SizeSetter {
    fn to_value(&self) -> usize {
        match self {
            SizeSetter::Value(_) => 0,
            SizeSetter::Grow(_) => 1,
            SizeSetter::Shrink(_) => 2,
            SizeSetter::Clamp { .. } => 3,
        }
    }

    fn from_value(value: usize) -> Self {
        match value {
            0 => SizeSetter::Value(Size::new(1.0, 1.0)),
            1 => SizeSetter::Grow(SizeDifference::Value(Size::new(100.0, 100.0))),
            2 => SizeSetter::Shrink(SizeDifference::Value(Size::new(100.0, 100.0))),
            3 => SizeSetter::Clamp {
                min: Size::new(0.0, 0.0),
                max: Size::new(100.0, 100.0),
            },
            _ => unreachable!(),
        }
    }

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F) {
        let setter_types = [
            im_str!("Value"),
            im_str!("Grow"),
            im_str!("Shrink"),
            im_str!("Clamp"),
        ];

        f(self, im_str!("Size Setter"), &setter_types);
    }
}

impl EnumSetters for SizeDifference {
    fn to_value(&self) -> usize {
        match self {
            SizeDifference::Value(_) => 0,
            SizeDifference::Percent(_) => 1,
        }
    }

    fn from_value(value: usize) -> Self {
        match value {
            0 => SizeDifference::Value(Size::new(0.0, 0.0)),
            1 => SizeDifference::Percent(Size::new(0.0, 0.0)),
            _ => unreachable!(),
        }
    }

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F) {
        let setter_types = [im_str!("Value"), im_str!("Percent")];

        f(self, im_str!("Size Difference"), &setter_types);
    }
}

impl EnumSetters for FlipSetter {
    fn to_value(&self) -> usize {
        match self {
            FlipSetter::Flip => 0,
            FlipSetter::SetFlip(_) => 1,
        }
    }

    fn from_value(value: usize) -> Self {
        match value {
            0 => FlipSetter::Flip,
            1 => FlipSetter::SetFlip(true),
            _ => unreachable!(),
        }
    }

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F) {
        let setter_types = [im_str!("Flip"), im_str!("Set Flip")];

        f(self, im_str!("Flip Setter"), &setter_types);
    }
}

impl EnumSetters for LayerSetter {
    fn to_value(&self) -> usize {
        match self {
            Self::Value(_) => 0,
            Self::Increase => 1,
            Self::Decrease => 2,
        }
    }

    fn from_value(value: usize) -> Self {
        match value {
            0 => Self::Value(0),
            1 => Self::Increase,
            2 => Self::Decrease,
            _ => unreachable!(),
        }
    }

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F) {
        let setter_types = [im_str!("Value"), im_str!("Increase"), im_str!("Decrease")];

        f(self, im_str!("Layer Setter"), &setter_types);
    }
}

impl EnumSetters for Speed {
    fn to_value(&self) -> usize {
        match self {
            Speed::VerySlow => 0,
            Speed::Slow => 1,
            Speed::Normal => 2,
            Speed::Fast => 3,
            Speed::VeryFast => 4,
            Speed::Value(_) => 5,
        }
    }

    fn from_value(value: usize) -> Self {
        match value {
            0 => Speed::VerySlow,
            1 => Speed::Slow,
            2 => Speed::Normal,
            3 => Speed::Fast,
            4 => Speed::VeryFast,
            5 => Speed::Value(0.0),
            _ => unreachable!(),
        }
    }

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F) {
        let types = [
            im_str!("Very Slow"),
            im_str!("Slow"),
            im_str!("Normal"),
            im_str!("Fast"),
            im_str!("Very Fast"),
            im_str!("Value"),
        ];

        f(self, im_str!("Animation Type"), &types);
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
    object: &mut SerialiseObject,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
) {
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
                    let first_key = choose_image_from_files(asset_files, images, path);
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
                        let first_key = choose_image_from_files(asset_files, images, path);
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
            colour.choose(ui);
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

    object.angle.choose_angle(ui);

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

pub fn rename_across_objects(objects: &mut Vec<SerialiseObject>, old_name: &str, new_name: &str) {
    for obj in objects.iter_mut() {
        rename_object(&mut obj.instructions, old_name, new_name);
    }
}

fn rename_object(instructions: &mut Vec<Instruction>, old_name: &str, new_name: &str) {
    let rename = |other_name: &mut String| {
        if *other_name == old_name {
            *other_name = new_name.to_string();
        }
    };
    for instruction in instructions.iter_mut() {
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
