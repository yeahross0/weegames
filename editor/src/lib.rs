#[macro_use]
extern crate imgui;

use imgui::{ImStr, ImString};
use imgui_opengl_renderer::Renderer as ImguiRenderer;
use imgui_sdl2::ImguiSdl2 as ImguiSdl;
use nfd::Response;
use sdl2::{
    event::Event, keyboard::Scancode, ttf::Sdl2TtfContext as TtfContext,
    video::Window as SdlWindow, EventPump, VideoSubsystem,
};
use sdlglue::{Model, Renderer, Texture};
use sfml::audio::SoundBuffer;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    process, str, thread,
    time::{Duration, Instant},
};

use wee::*;
use wee_common::{
    Colour, Flip, Rect, Size, Vec2, WeeResult, AABB, PROJECTION_HEIGHT, PROJECTION_WIDTH,
};

pub const SMALL_BUTTON: [f32; 2] = [100.0, 50.0];
pub const NORMAL_BUTTON: [f32; 2] = [200.0, 50.0];

pub enum EditorStatus {
    ReturnToMenu,
    None,
}

pub fn run_editor(
    renderer: &mut Renderer,
    event_pump: &mut EventPump,
    imgui: &mut Imgui,
    intro_font: &IntroFont,
    settings: GameSettings,
) -> WeeResult<()> {
    let mut last_frame = Instant::now();
    let mut editor_status = EditorStatus::None;
    let mut game = GameData::default();
    let mut assets = Assets::default();
    let mut filename = None;
    let mut game_position = Vec2::new(172.0, 106.0);
    let mut scale: f32 = 0.8;
    let mut minus_button = ButtonState::Up;
    let mut plus_button = ButtonState::Up;
    let mut show_collision_areas = false;
    let mut selected_index = None;
    let mut instruction_index = 0;
    let mut instruction_mode = InstructionMode::View;
    let mut instruction_focus = InstructionFocus::None;
    let mut animation_editor = AnimationEditor {
        new_sprite: Sprite::Colour(Colour::black()),
        index: 0,
    };
    struct RenameObject {
        index: usize,
        name: String,
        buffer: ImString,
    }
    let mut rename_object: Option<RenameObject> = None;
    let mut new_object = SerialiseObject::default();
    let mut new_name_buffer = ImString::from("".to_string());

    let mut fonts_window_opened = false;
    let mut background_window_opened = false;
    let mut objects_window_opened = true;
    let mut main_window_opened = true;
    let mut demo_window_opened = false;

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
        //sdlglue::set_fullscreen(renderer, event_pump)?;

        let imgui_frame =
            imgui.prepare_frame(&renderer.window, &event_pump.mouse_state(), &mut last_frame);
        let ui = &imgui_frame.ui;

        if demo_window_opened {
            ui.show_demo_window(&mut demo_window_opened);
        }

        let menu_bar = ui.begin_main_menu_bar();
        if let Some(bar) = menu_bar {
            let menu = ui.begin_menu(im_str!("File"), true);
            if let Some(menu) = menu {
                if imgui::MenuItem::new(im_str!("New")).build(&ui) {
                    game = GameData::default();
                    assets = Assets::default();
                    filename = None;
                }
                if imgui::MenuItem::new(im_str!("Open")).build(&ui) {
                    let games_path = std::env::current_dir().unwrap().join(Path::new("games"));
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

                                        assets = match Assets::load(
                                            &game.asset_files,
                                            &file_path,
                                            intro_font.ttf_context,
                                        ) {
                                            Ok(assets) => assets,
                                            Err(error) => {
                                                menu.end(ui);
                                                return Err(error);
                                            }
                                        };
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
                if imgui::MenuItem::new(im_str!("Save")).build(&ui) {
                    match &filename {
                        Some(filename) => {
                            let s = serde_json::to_string_pretty(&game);
                            match s {
                                Ok(s) => {
                                    std::fs::write(&filename, s)
                                        .unwrap_or_else(|e| log::error!("{}", e));
                                    println!("SAVED! {}", filename);
                                }
                                Err(error) => {
                                    log::error!("{}", error);
                                }
                            }
                        }
                        None => {
                            // TODO:
                            println!("TODO!");
                        }
                    }
                }
                if imgui::MenuItem::new(im_str!("Save As")).build(&ui) {
                    let games_path = std::env::current_dir().unwrap().join(Path::new("games"));
                    let response = nfd::open_save_dialog(None, games_path.to_str());
                    match response {
                        Ok(response) => {
                            if let Response::Okay(file_path) = response {
                                log::info!("File path = {:?}", file_path);
                                // let final_path = base_file_path.canonfile_path
                                let s = serde_json::to_string_pretty(&game);
                                match s {
                                    Ok(s) => {
                                        std::fs::write(&file_path, s)
                                            .unwrap_or_else(|e| log::error!("{}", e));
                                        filename = Some(file_path);
                                    }
                                    Err(error) => {
                                        log::error!("{}", error);
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            log::error!("{}", error);
                        }
                    }
                }
                if imgui::MenuItem::new(im_str!("Return to Menu")).build(ui) {
                    editor_status = EditorStatus::ReturnToMenu;
                }
                menu.end(ui);
            }

            let toggle = |label, opened: &mut bool| {
                let mut toggled = *opened;
                if imgui::MenuItem::new(label).build_with_ref(ui, &mut toggled) {
                    *opened = !(*opened);
                }
            };

            let menu = ui.begin_menu(im_str!("View"), true);
            if let Some(menu) = menu {
                toggle(im_str!("Main Window"), &mut main_window_opened);
                toggle(im_str!("Objects"), &mut objects_window_opened);
                //toggle(im_str!("Sound FX"), &mut windows.sound_fx.opened);
                //toggle(im_str!("Music"), &mut windows.music.opened);
                toggle(im_str!("Background"), &mut background_window_opened);
                toggle(im_str!("Fonts"), &mut fonts_window_opened);
                toggle(im_str!("Demo Window"), &mut demo_window_opened);
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

        let mut play_game = false;
        if main_window_opened {
            imgui::Window::new(im_str!("Main Window"))
                .size(window_size, imgui::Condition::FirstUseEver)
                .scroll_bar(true)
                .scrollable(true)
                .resizable(true)
                .opened(&mut main_window_opened)
                .build(&ui, || {
                    if ui.button(im_str!("Play"), NORMAL_BUTTON) {
                        play_game = true;
                    }
                    imgui::Slider::new(
                        im_str!("Game Length"),
                        std::ops::RangeInclusive::new(2.0, 8.0),
                    )
                    .display_format(im_str!("%.1f"))
                    .build(&ui, &mut game.length);
                });
        }

        if play_game {
            let completed_game = LoadedGame::with_assets(game.clone(), assets, &intro_font)?
                .start(1.0, settings)
                .play(renderer, event_pump)?;
            assets = completed_game.assets;
        }

        if objects_window_opened {
            imgui::Window::new(im_str!("Objects"))
                .size(window_size, imgui::Condition::FirstUseEver)
                .position([600.0, 60.0], imgui::Condition::FirstUseEver)
                .scroll_bar(true)
                .scrollable(true)
                .resizable(true)
                .opened(&mut objects_window_opened)
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
                                Add,
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
                            let is_being_renamed = |rename_object: &Option<RenameObject>, index| {
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
                                    if imgui::Selectable::new(&im_str!("{}", game.objects[i].name))
                                        .selected(selected_index == Some(i))
                                        .build(&ui)
                                    {
                                        selected_index = Some(i);
                                        instruction_mode = InstructionMode::View;
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

                            if ui.small_button(im_str!("New Object")) {
                                object_operation = ObjectOperation::Add;
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
                                        name: game.objects[selected_index.unwrap()].name.clone(),
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
                                        let next_index = (*index + 1).min(game.objects.len() - 1);
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
                                ObjectOperation::Add => {
                                    new_object = SerialiseObject::default();
                                    new_name_buffer = ImString::from("".to_string());
                                    ui.open_popup(im_str!("Create Object"));
                                    //game.objects.push(SerialiseObject::default());
                                    /*rename_object = Some(RenameObject {
                                        index: game.objects.len() - 1,
                                        name: "".to_string(),
                                        buffer: ImString::from("".to_string()),
                                    });*/
                                }
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

                            ui.popup_modal(im_str!("Create Object")).build(|| {
                                let entered = ui
                                    .input_text(im_str!("Name"), &mut new_name_buffer)
                                    .resize_buffer(true)
                                    .enter_returns_true(true)
                                    .build();

                                choose_sprite(
                                    &mut new_object.sprite,
                                    &ui,
                                    &mut game.asset_files,
                                    &mut assets.images,
                                    &filename,
                                );
                                if let Sprite::Image { name } = &new_object.sprite {
                                    new_object.size.width = assets.images[name].width as f32;
                                    new_object.size.height = assets.images[name].height as f32;
                                }

                                if entered || ui.button(im_str!("OK"), NORMAL_BUTTON) {
                                    let duplicate =
                                        game.objects.iter().any(|o| o.name == new_object.name);
                                    if duplicate {
                                        ui.open_popup(im_str!("Duplicate Name"));
                                    } else {
                                        ui.close_current_popup();
                                    }
                                    new_object.name = new_name_buffer.to_str().to_string();
                                    game.objects.push(new_object.clone());
                                    selected_index = Some(game.objects.len() - 1);
                                    ui.close_current_popup();
                                }
                                ui.same_line(0.0);
                                if ui.button(im_str!("Cancel"), NORMAL_BUTTON) {
                                    ui.close_current_popup();
                                }
                            });

                            ui.popup_modal(im_str!("Duplicate Name")).build(|| {
                                ui.text(im_str!("An object with this name already exists"));
                                if ui.button(im_str!("OK"), NORMAL_BUTTON) {
                                    ui.close_current_popup();
                                }
                            });
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
                            &mut assets,
                            intro_font.ttf_context,
                            game.length,
                            &mut animation_editor,
                            &filename,
                            &mut instruction_mode,
                            &mut instruction_index,
                            &mut instruction_focus,
                        );
                        game.objects[index] = object;
                    }
                });
        }

        if background_window_opened {
            // TODO: Fix up
            imgui::Window::new(im_str!("Background"))
                .size(window_size, imgui::Condition::FirstUseEver)
                .opened(&mut background_window_opened)
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

                            ui.input_float(im_str!("Min X"), &mut game.background[i].area.min.x)
                                .build();
                            ui.input_float(im_str!("Min Y"), &mut game.background[i].area.min.y)
                                .build();
                            ui.input_float(im_str!("Max X"), &mut game.background[i].area.max.x)
                                .build();
                            ui.input_float(im_str!("Max Y"), &mut game.background[i].area.max.y)
                                .build();
                        }

                        stack.pop(&ui);
                    }
                    if ui.button(im_str!("New Background"), NORMAL_BUTTON) {
                        if assets.images.is_empty() {
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
                        if assets.images.is_empty() {
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
        }

        if fonts_window_opened {
            imgui::Window::new(im_str!("Fonts"))
                .size(window_size, imgui::Condition::FirstUseEver)
                .menu_bar(true)
                .resizable(true)
                .opened(&mut fonts_window_opened)
                .build(ui, || {
                    let menu_bar = ui.begin_menu_bar();

                    if let Some(bar) = menu_bar {
                        let menu = ui.begin_menu(im_str!("File"), true);
                        if let Some(menu) = menu {
                            if imgui::MenuItem::new(im_str!("Add Fonts")).build(&ui) {
                                let path = match &filename {
                                    Some(filename) => {
                                        Path::new(filename).parent().unwrap().join("fonts")
                                    }
                                    None => Path::new("games").to_owned(),
                                };
                                choose_font_from_files(
                                    &mut game.asset_files.fonts,
                                    &mut assets.fonts,
                                    path,
                                    intro_font.ttf_context,
                                );
                            }
                            menu.end(&ui);
                        }

                        bar.end(&ui);
                    }

                    for (name, font) in game.asset_files.fonts.iter_mut() {
                        ui.text(name);
                        let base_path = match &filename {
                            Some(filename) => Path::new(filename).parent().unwrap().join("fonts"),
                            None => Path::new("games").to_owned(),
                        };
                        let path = base_path.join(&font.filename);
                        // TODO: Min Max this
                        if ui.input_float(im_str!("Size"), &mut font.size).build() {
                            *assets.fonts.get_mut(name).unwrap() = intro_font
                                .ttf_context
                                .load_font(&path, font.size as u16)
                                .unwrap();
                        }
                    }
                });
        }

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
                        let texture = assets.images.get_image(name)?;

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
                                let texture = assets.images.get_image(image_name)?;
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
                                renderer.fill_rectangle(model, Colour::rgba(0.0, 1.0, 0.0, 0.5));
                            }

                            let aabb = game_object.collision_aabb();
                            let mut origin = game_object.origin();
                            if let Some(area) = game_object.collision_area {
                                origin = Vec2::new(origin.x - area.min.x, origin.y - area.min.y);
                            }
                            let rect = aabb.to_rect().move_position(game_position).scale(scale);
                            let model = Model::new(
                                rect,
                                Some(origin * scale),
                                object.angle,
                                Flip::default(),
                            );
                            // TODO: model.move().scale()
                            renderer.fill_rectangle(model, Colour::rgba(1.0, 0.0, 0.0, 0.5));
                        }
                    }
                }
            }
        }

        imgui_frame.render(&renderer.window);
        renderer.present();

        thread::sleep(Duration::from_millis(10));

        if let EditorStatus::ReturnToMenu = editor_status {
            break 'editor_running;
        }
    }

    Ok(())
}

pub struct Imgui {
    context: imgui::Context,
    sdl: ImguiSdl,
    renderer: ImguiRenderer,
}

impl Imgui {
    pub fn init(
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

    pub fn prepare_frame(
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

pub struct ImguiFrame<'a> {
    pub ui: imgui::Ui<'a>,
    sdl: &'a mut ImguiSdl,
    renderer: &'a mut ImguiRenderer,
}

impl<'a> ImguiFrame<'a> {
    pub fn render(self, window: &SdlWindow) {
        self.sdl.prepare_render(&self.ui, window);
        self.renderer.render(self.ui);
    }
}

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
                            imgui::ColorButton::new(
                                im_str!("##Colour"),
                                [colour.r, colour.g, colour.b, colour.a],
                            )
                            .size([80.0, 80.0])
                            .build(&ui);
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
                    if ui.is_item_hovered() {
                        ui.tooltip(|| {
                            imgui::ColorButton::new(
                                im_str!("##Colour"),
                                [colour.r, colour.g, colour.b, colour.a],
                            )
                            .size([80.0, 80.0])
                            .build(&ui);
                        })
                    };
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

fn right_window<'a, 'b>(
    ui: &imgui::Ui,
    object: &mut SerialiseObject,
    object_names: &Vec<&str>,
    sprites: &HashMap<&str, &Sprite>,
    asset_files: &mut AssetFiles,
    assets: &mut Assets<'a, 'b>,
    ttf_context: &'a TtfContext,
    game_length: f32,
    animation_editor: &mut AnimationEditor,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
    instruction_index: &mut usize,
    instruction_focus: &mut InstructionFocus,
) {
    imgui::ChildWindow::new(im_str!("Right"))
        .size([0.0, 0.0])
        .scroll_bar(true)
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
                    properties_window(ui, object, asset_files, &mut assets.images, filename);
                });
                tab_item(im_str!("Instructions"), || {
                    choose_instructions(
                        &mut object.instructions,
                        ui,
                        object_names,
                        sprites,
                        asset_files,
                        assets,
                        ttf_context,
                        game_length,
                        animation_editor,
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

fn choose_instruction(
    instruction: &mut Instruction,
    ui: &imgui::Ui,
    instruction_mode: &mut InstructionMode,
    focus: &mut InstructionFocus,
) {
    ui.text("Triggers:");
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

trait Choose {
    fn choose(&mut self, ui: &imgui::Ui);
}

fn choose_when(when: &mut When, ui: &imgui::Ui, game_length: f32) {
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
                .max((game_length * 60.0) as i32)
                .build();
            *start = frames[0] as u32;
            *end = frames[1] as u32;
        }
        _ => {}
    }
}

fn choose_collision_with(with: &mut CollisionWith, ui: &imgui::Ui, object_names: &Vec<&str>) {
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
                name: object_names[0].to_string(),
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

fn choose_mouse_over(over: &mut MouseOver, ui: &imgui::Ui, object_names: &Vec<&str>) {
    let mut input_type = match over {
        MouseOver::Object { .. } => 0,
        MouseOver::Area(_) => 1,
        MouseOver::Anywhere => 2,
    };
    let input_typename = if input_type == 0 {
        "Object".to_string()
    } else if input_type == 1 {
        "Area".to_string()
    } else {
        "Anywhere".to_string()
    };
    if imgui::Slider::new(im_str!("Mouse Over"), std::ops::RangeInclusive::new(0, 2))
        .display_format(&ImString::from(input_typename))
        .build(&ui, &mut input_type)
    {
        *over = if input_type == 0 {
            MouseOver::Object {
                name: object_names[0].to_string(),
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

fn choose_percent(percent: &mut f32, ui: &imgui::Ui) {
    let mut chance_percent = *percent * 100.0;
    ui.drag_float(im_str!("Chance"), &mut chance_percent)
        .min(0.0)
        .max(100.0)
        .speed(1.0)
        .display_format(im_str!("%.01f%%"))
        .build();
    *percent = chance_percent / 100.0;
}

fn choose_object(object: &mut String, ui: &imgui::Ui, object_names: &Vec<&str>) {
    let mut current_object = object_names
        .iter()
        .position(|obj_name| obj_name == object)
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
        *object = object_names[current_object].to_string();
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

fn select_sprite_type(
    sprite: &mut Sprite,
    ui: &imgui::Ui,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
) {
    let is_sprite = if let Sprite::Image { .. } = &sprite {
        true
    } else {
        false
    };
    if ui.radio_button_bool(im_str!("Sprite"), is_sprite) {
        match images.keys().next() {
            Some(name) => *sprite = Sprite::Image { name: name.clone() },
            None => {
                let path = match filename {
                    Some(filename) => Path::new(filename).parent().unwrap().join("images"),
                    None => Path::new("games").to_owned(),
                };
                let image = choose_image_from_files(asset_files, images, path);
                if let Some(image) = image {
                    *sprite = Sprite::Image { name: image };
                }
            }
        }
    }
    ui.same_line(0.0);
    if ui.radio_button_bool(im_str!("Colour"), !is_sprite) {
        *sprite = Sprite::Colour(Colour::black());
    }
}

fn choose_new_image<P: AsRef<Path>>(
    image: &mut Sprite,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    path: P,
) {
    let first_key = choose_image_from_files(asset_files, images, path);
    match first_key {
        Some(key) => {
            *image = Sprite::Image { name: key.clone() };
        }
        None => {
            log::error!("None of the new images loaded correctly");
        }
    }
}

fn choose_font<'a, 'b>(
    font_name: &mut String,
    ui: &imgui::Ui,
    font_files: &mut HashMap<String, FontLoadInfo>,
    fonts: &mut Fonts<'a, 'b>,
    ttf_context: &'a TtfContext,
    filename: &Option<String>,
) {
    let path = match filename {
        Some(filename) => Path::new(filename).parent().unwrap().join("fonts"),
        None => Path::new("games").to_owned(),
    };

    if fonts.is_empty() {
        if ui.button(im_str!("Add a New Font"), NORMAL_BUTTON) {
            let first_key = choose_font_from_files(font_files, fonts, path, ttf_context);
            match first_key {
                Some(key) => {
                    *font_name = key.clone();
                }
                None => {
                    // TODO: What if user clicks cancel
                    log::error!("None of the new fonts loaded correctly");
                }
            }
        }
    } else {
        let mut current_font = fonts.keys().position(|k| k == font_name).unwrap_or(0);
        let keys = {
            let mut keys: Vec<ImString> = fonts.keys().map(|k| ImString::from(k.clone())).collect();

            keys.push(ImString::new("Add a New Font"));

            keys
        };

        let font_names: Vec<&ImString> = keys.iter().collect();

        if imgui::ComboBox::new(im_str!("Fonts")).build_simple_string(
            &ui,
            &mut current_font,
            &font_names,
        ) {
            if current_font == font_names.len() - 1 {
                let first_key = choose_font_from_files(font_files, fonts, path, ttf_context);
                match first_key {
                    Some(key) => {
                        *font_name = key.clone();
                    }
                    None => {
                        log::error!("None of the new fonts loaded correctly");
                    }
                }
            } else {
                match fonts.keys().nth(current_font) {
                    Some(font) => *font_name = font.clone(),
                    None => {
                        log::error!("Could not set font to index {}", current_font);
                    }
                }
            }
        }
    };
}

fn choose_sound(
    sound_name: &mut String,
    ui: &imgui::Ui,
    audio_filenames: &mut HashMap<String, String>,
    sounds: &mut Sounds,
    filename: &Option<String>,
) {
    let path = match filename {
        Some(filename) => Path::new(filename).parent().unwrap().join("audio"),
        None => Path::new("games").to_owned(),
    };

    if sounds.is_empty() {
        if ui.button(im_str!("Add a New Sound"), NORMAL_BUTTON) {
            let first_key = choose_sound_from_files(audio_filenames, sounds, path);
            match first_key {
                Some(key) => {
                    *sound_name = key.clone();
                }
                None => {
                    log::error!("None of the new sounds loaded correctly");
                }
            }
        }
    } else {
        let mut current_sound = sounds.keys().position(|k| k == sound_name).unwrap_or(0);
        let keys = {
            let mut keys: Vec<ImString> =
                sounds.keys().map(|k| ImString::from(k.clone())).collect();

            keys.push(ImString::new("Add a New Sound"));

            keys
        };

        let font_names: Vec<&ImString> = keys.iter().collect();

        if imgui::ComboBox::new(im_str!("Sound")).build_simple_string(
            &ui,
            &mut current_sound,
            &font_names,
        ) {
            if current_sound == font_names.len() - 1 {
                let first_key = choose_sound_from_files(audio_filenames, sounds, path);
                match first_key {
                    Some(key) => {
                        *sound_name = key.clone();
                    }
                    None => {
                        log::error!("None of the new sounds loaded correctly");
                    }
                }
            } else {
                match sounds.keys().nth(current_sound) {
                    Some(sound) => *sound_name = sound.clone(),
                    None => {
                        log::error!("Could not set sound to index {}", current_sound);
                    }
                }
            }
        }
    };
}

struct AnimationEditor {
    new_sprite: Sprite,
    index: usize,
}

fn choose_animation(
    animation: &mut Vec<Sprite>,
    ui: &imgui::Ui,
    editor: &mut AnimationEditor,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
) {
    for (index, sprite) in animation.iter().enumerate() {
        match sprite {
            Sprite::Image { name } => {
                let image_id = images[name].id;
                if imgui::ImageButton::new(
                    imgui::TextureId::from(image_id as usize),
                    [100.0, 100.0],
                )
                //.tint_col([0.9, 0.0, 0.0, 1.0])
                //.background_col([1.0, 0.0, 0.0, 0.5])
                .build(ui)
                {
                    ui.open_popup(im_str!("Edit Animation"));
                    editor.index = index;
                }
            }
            Sprite::Colour(colour) => {
                if imgui::ColorButton::new(
                    im_str!("##Colour"),
                    [colour.r, colour.g, colour.b, colour.a],
                )
                .size([108.0, 108.0])
                .build(&ui)
                {
                    ui.open_popup(im_str!("Edit Animation"));
                    editor.index = index;
                }
            }
        }
    }

    enum AnimationTask {
        MoveBefore,
        MoveAfter,
        Delete,
        None,
    };
    let mut task = AnimationTask::None;
    ui.popup(im_str!("Edit Animation"), || {
        if imgui::Selectable::new(im_str!("Move Before")).build(ui) {
            task = AnimationTask::MoveBefore;
        }
        if imgui::Selectable::new(im_str!("Move After")).build(ui) {
            task = AnimationTask::MoveAfter;
        }
        if imgui::Selectable::new(im_str!("Delete")).build(ui) {
            task = AnimationTask::Delete;
        }
    });

    match task {
        AnimationTask::MoveBefore => {
            if editor.index > 0 {
                let tmp = animation[editor.index].clone();
                animation[editor.index] = animation[editor.index - 1].clone();
                animation[editor.index - 1] = tmp;
            }
        }
        AnimationTask::MoveAfter => {
            if editor.index < animation.len() - 1 {
                let tmp = animation[editor.index].clone();
                animation[editor.index] = animation[editor.index + 1].clone();
                animation[editor.index + 1] = tmp;
            }
        }
        AnimationTask::Delete => {
            log::info!("{}", editor.index);
            animation.remove(editor.index);
        }
        AnimationTask::None => {}
    }

    if ui.button(im_str!("Add Sprite"), NORMAL_BUTTON) {
        ui.open_popup(im_str!("Add Animation Sprite"));
        editor.new_sprite = Sprite::Colour(Colour::black());
    }
    ui.popup(im_str!("Add Animation Sprite"), || {
        choose_sprite(&mut editor.new_sprite, ui, asset_files, images, filename);

        if ui.button(im_str!("OK"), NORMAL_BUTTON) {
            animation.push(editor.new_sprite.clone());
            ui.close_current_popup();
        }
    });
}

fn choose_sprite(
    sprite: &mut Sprite,
    ui: &imgui::Ui,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
) {
    select_sprite_type(sprite, ui, asset_files, images, filename);

    match sprite {
        Sprite::Image { name: image_name } => {
            // TODO: Tidy up
            let path = match filename {
                Some(filename) => Path::new(filename).parent().unwrap().join("images"),
                None => Path::new("games").to_owned(),
            };

            if images.is_empty() {
                if ui.button(im_str!("Add a New Image"), NORMAL_BUTTON) {
                    choose_new_image(sprite, asset_files, images, path);
                }
            } else {
                let mut current_image = images.keys().position(|k| k == image_name).unwrap_or(0);
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
                        choose_new_image(sprite, asset_files, images, path);
                    } else {
                        match images.keys().nth(current_image) {
                            Some(image) => {
                                *sprite = Sprite::Image {
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

fn choose_property_check(
    property: &mut PropertyCheck,
    ui: &imgui::Ui,
    images: &mut Images,
    asset_files: &mut AssetFiles,
    filename: &Option<String>,
) {
    match property {
        PropertyCheck::Switch(switch_state) => {
            switch_state.choose(ui);
        }
        PropertyCheck::Sprite(sprite) => {
            choose_sprite(sprite, ui, asset_files, images, filename);
        }
        _ => {}
    }
}

fn choose_trigger(
    trigger: &mut Trigger,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    game_length: f32,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
) {
    let first_name = || object_names[0].to_string();
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
    match trigger {
        Trigger::Time(when) => {
            choose_when(when, ui, game_length);
        }
        Trigger::Collision(with) => {
            choose_collision_with(with, ui, object_names);
        }
        Trigger::Input(Input::Mouse { over, interaction }) => {
            choose_mouse_over(over, ui, object_names);
            interaction.choose(ui);
        }
        Trigger::WinStatus(status) => {
            status.choose(ui);
        }
        Trigger::Random { chance } => {
            choose_percent(chance, ui);
        }
        Trigger::CheckProperty { name, check } => {
            choose_object(name, ui, object_names);
            choose_property_check(check, ui, images, asset_files, filename);
        }
    }
    if ui.small_button(im_str!("Back")) {
        *instruction_mode = InstructionMode::Edit;
    }
}

fn choose_action<'a, 'b>(
    action: &mut Action,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    asset_files: &mut AssetFiles,
    assets: &mut Assets<'a, 'b>,
    ttf_context: &'a TtfContext,
    animation_editor: &mut AnimationEditor,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
) {
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

    match action {
        Action::Effect(effect) => {
            effect.choose(ui);
        }
        Action::Motion(motion) => {
            choose_motion(motion, ui, object_names);
        }
        Action::PlaySound { name } => {
            choose_sound(
                name,
                ui,
                &mut asset_files.audio,
                &mut assets.sounds,
                filename,
            );
        }
        Action::SetProperty(setter) => {
            choose(
                setter,
                ui,
                object_names,
                &mut assets.images,
                asset_files,
                filename,
            );
        }
        Action::Animate {
            animation_type,
            sprites,
            speed,
        } => {
            animation_type.choose(ui);
            choose_animation(
                sprites,
                ui,
                animation_editor,
                asset_files,
                &mut assets.images,
                filename,
            );
            speed.choose(ui);
        }
        Action::DrawText {
            text,
            font,
            colour,
            resize,
        } => {
            text.choose(ui);
            choose_font(
                font,
                ui,
                &mut asset_files.fonts,
                &mut assets.fonts,
                ttf_context,
                filename,
            );
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

fn choose(
    property: &mut PropertySetter,
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
    let mut current_property_position = match property {
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
        *property = match current_property_position {
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
    match property {
        PropertySetter::Sprite(sprite) => {
            choose_sprite(sprite, ui, asset_files, images, filename);
        }
        PropertySetter::Angle(angle_setter) => {
            choose_angle_setter(angle_setter, ui, object_names);
        }
        PropertySetter::Size(size_setter) => {
            size_setter.choose(ui);
        }
        PropertySetter::Switch(switch) => {
            switch.choose(ui);
        }
        PropertySetter::Timer { time } => {
            choose_u32(time, ui, im_str!("Time"));
        }
        PropertySetter::FlipHorizontal(flip_setter) | PropertySetter::FlipVertical(flip_setter) => {
            flip_setter.choose(ui);
        }
        PropertySetter::Layer(layer_setter) => {
            layer_setter.choose(ui);
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

fn choose_u32(value: &mut u32, ui: &imgui::Ui, label: &ImStr) {
    let mut changed_value = *value as i32;
    ui.input_int(label, &mut changed_value).build();
    *value = changed_value as u32;
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

fn choose_angle_setter(setter: &mut AngleSetter, ui: &imgui::Ui, object_names: &Vec<&str>) {
    let angle_types = [
        im_str!("Value"),
        im_str!("Increase"),
        im_str!("Decrease"),
        im_str!("Match"),
        im_str!("Clamp"),
        im_str!("Rotate To Mouse"),
    ];
    let mut current_angle_position = match setter {
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
        *setter = match current_angle_position {
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

    match setter {
        AngleSetter::Value(value) => {
            choose_angle(value, ui);
        }
        AngleSetter::Increase(value) => {
            ui.input_float(im_str!("Angle"), value).build();
        }
        AngleSetter::Decrease(value) => {
            ui.input_float(im_str!("Angle"), value).build();
        }
        AngleSetter::Match { name } => {
            choose_object(name, ui, object_names);
        }
        AngleSetter::Clamp { min, max } => {
            ui.input_float(im_str!("Min Angle"), min).build();
            ui.input_float(im_str!("Max Angle"), max).build();
        }
        _ => {}
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

fn choose_motion(motion: &mut Motion, ui: &imgui::Ui, object_names: &Vec<&str>) {
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

    match motion {
        Motion::GoStraight { direction, speed } | Motion::Accelerate { direction, speed } => {
            direction.choose(ui);
            speed.choose(ui);
        }
        Motion::JumpTo(jump_location) => {
            choose_jump_location(jump_location, ui, object_names);
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
            choose_object(name, ui, object_names);
        }
        Motion::Target {
            target,
            target_type,
            offset,
            speed,
        } => {
            choose_target(target, ui, object_names);
            target_type.choose(ui);

            ui.input_float(im_str!("Offset X"), &mut offset.x).build();
            ui.input_float(im_str!("Offset Y"), &mut offset.y).build();

            speed.choose(ui);
        }
        _ => {}
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
                choose_angle(angle, ui);
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

fn choose_angle(angle: &mut f32, ui: &imgui::Ui) {
    let mut radians = angle.to_radians();
    imgui::AngleSlider::new(im_str!("Angle"))
        .min_degrees(-360.0)
        .max_degrees(360.0)
        .build(ui, &mut radians);
    *angle = radians.to_degrees();

    if ui.is_item_hovered() && ui.is_mouse_released(imgui::MouseButton::Right) {
        ui.open_popup(im_str!("Set specific angle"));
    }

    ui.popup(im_str!("Set specific angle"), || {
        ui.input_float(im_str!("Angle"), angle).build();
    });
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
        ui.text("Random directions:");
        let mut selectable = |direction| {
            let contains_direction = self.contains(&direction);
            if imgui::Selectable::new(&im_str!("{}", direction))
                .selected(contains_direction)
                .size([100.0, 100.0])
                .build(ui)
            {
                if contains_direction {
                    self.remove(&direction);
                } else {
                    self.insert(direction);
                }
            }
        };

        selectable(CompassDirection::UpLeft);
        ui.same_line(0.0);
        selectable(CompassDirection::Up);
        ui.same_line(0.0);
        selectable(CompassDirection::UpRight);

        selectable(CompassDirection::Left);
        ui.same_line(216.0);
        selectable(CompassDirection::Right);

        selectable(CompassDirection::DownLeft);
        ui.same_line(0.0);
        selectable(CompassDirection::Down);
        ui.same_line(0.0);
        selectable(CompassDirection::DownRight);
    }
}

fn choose_jump_location(jump: &mut JumpLocation, ui: &imgui::Ui, object_names: &Vec<&str>) {
    let jump_types = [
        im_str!("Point"),
        im_str!("Area"),
        im_str!("Mouse"),
        im_str!("Object"),
        im_str!("Relative"),
        im_str!("Clamp Position"),
    ];
    let mut current_jump_position = match jump {
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
        *jump = match current_jump_position {
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

    match jump {
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

                initial_direction.choose(ui);

                /*match initial_direction {
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
                }*/
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

fn choose_target(target: &mut Target, ui: &imgui::Ui, object_names: &Vec<&str>) {
    if ui.radio_button_bool(im_str!("Target Object"), *target != Target::Mouse) {
        *target = Target::Object {
            name: object_names[0].to_string(),
        };
    }
    if ui.radio_button_bool(im_str!("Target Mouse"), *target == Target::Mouse) {
        *target = Target::Mouse;
    }
    if let Target::Object { name } = target {
        choose_object(name, ui, object_names);
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

fn choose_instructions<'a, 'b>(
    instructions: &mut Vec<Instruction>,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    sprites: &HashMap<&str, &Sprite>,
    asset_files: &mut AssetFiles,
    assets: &mut Assets<'a, 'b>,
    ttf_context: &'a TtfContext,
    game_length: f32,
    animation_editor: &mut AnimationEditor,
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
                    for (i, instruction) in instructions.iter_mut().enumerate() {
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
                                        trigger.display(ui, sprites, &assets.images);
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
                instructions.swap(*selected_index, *selected_index - 1);
                *selected_index -= 1;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Down")) && *selected_index + 1 < instructions.len() {
                instructions.swap(*selected_index, *selected_index + 1);
                *selected_index += 1;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Add")) {
                instructions.push(wee::Instruction {
                    triggers: Vec::new(),
                    actions: Vec::new(),
                });
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Edit")) {
                *instruction_mode = InstructionMode::Edit;
            }
            ui.same_line(0.0);
            if ui.small_button(im_str!("Delete")) && !instructions.is_empty() {
                instructions.remove(*selected_index);
                if *selected_index > 0 {
                    *selected_index -= 1;
                }
            }
        }
        InstructionMode::Edit => {
            choose_instruction(
                &mut instructions[*selected_index],
                ui,
                instruction_mode,
                focus,
            );
        }
        InstructionMode::AddTrigger => {
            let instruction = &mut instructions[*selected_index];
            instruction.triggers.push(Trigger::Time(When::Start));
            *focus = InstructionFocus::Trigger {
                index: instruction.triggers.len() - 1,
            };
            *instruction_mode = InstructionMode::EditTrigger;
        }
        InstructionMode::AddAction => {
            let instruction = &mut instructions[*selected_index];
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
            choose_trigger(
                &mut instructions[*selected_index].triggers[trigger_index],
                ui,
                object_names,
                game_length,
                asset_files,
                &mut assets.images,
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
            choose_action(
                &mut instructions[*selected_index].actions[action_index],
                ui,
                object_names,
                asset_files,
                assets,
                ttf_context,
                animation_editor,
                filename,
                instruction_mode,
            );
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
        if ui.radio_button_bool(label, self.to_value() == 0) {
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

        f(self, im_str!("Speed Type"), &types);
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

fn load_sounds(
    audio_files: &mut HashMap<String, String>,
    sounds: &mut Sounds,
    sound_filenames: Vec<(WeeResult<String>, String)>,
) -> Option<String> {
    let mut first_key = None;
    for (key, path) in &sound_filenames {
        match key {
            Ok(key) => {
                let sound = SoundBuffer::from_file(path);
                match sound {
                    Some(sound) => {
                        sounds.insert(key.clone(), sound);
                        let filename = Path::new(path)
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string();
                        audio_files.insert(key.clone(), filename);
                        first_key = Some(key.clone());
                    }
                    None => {
                        log::error!("Could not add sound with filename {}", path);
                    }
                }
            }
            Err(error) => {
                log::error!("Could not add sound with filename {}", path);
                log::error!("{}", error);
            }
        }
    }
    first_key
}

fn load_fonts<'a, 'b>(
    font_files: &mut HashMap<String, FontLoadInfo>,
    fonts: &mut Fonts<'a, 'b>,
    font_filenames: Vec<(WeeResult<String>, String)>,
    ttf_context: &'a TtfContext,
) -> Option<String> {
    let mut first_key = None;
    for (key, path) in &font_filenames {
        match key {
            Ok(key) => {
                let font = ttf_context.load_font(&path, 128);
                match font {
                    Ok(font) => {
                        fonts.insert(key.clone(), font);
                        let filename = Path::new(&path)
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .to_string();
                        let load_info = FontLoadInfo {
                            filename,
                            size: 128.0,
                        };
                        font_files.insert(key.clone(), load_info);
                        first_key = Some(key.clone());
                    }
                    Err(error) => {
                        log::error!("Could not add font with filename {}", path);
                        log::error!("{}", error);
                    }
                }
            }
            Err(error) => {
                log::error!("Could not add font with filename {}", path);
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

fn choose_sound_from_files<P: AsRef<Path>>(
    audio_files: &mut HashMap<String, String>,
    sounds: &mut Sounds,
    sounds_path: P,
) -> Option<String> {
    let result = nfd::open_file_multiple_dialog(None, sounds_path.as_ref().to_str()).unwrap();

    match result {
        Response::Okay(_) => {
            unreachable!();
        }
        Response::OkayMultiple(files) => {
            log::info!("Files {:?}", files);
            let sound_filenames: Vec<(WeeResult<String>, String)> = files
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

            load_sounds(audio_files, sounds, sound_filenames)
        }
        _ => None,
    }
}

fn choose_font_from_files<'a, 'b, P: AsRef<Path>>(
    font_files: &mut HashMap<String, FontLoadInfo>,
    fonts: &mut Fonts<'a, 'b>,
    fonts_path: P,
    ttf_context: &'a TtfContext,
) -> Option<String> {
    let result = nfd::open_file_multiple_dialog(None, fonts_path.as_ref().to_str()).unwrap();

    match result {
        Response::Okay(_) => {
            unreachable!();
        }
        Response::OkayMultiple(files) => {
            log::info!("Files {:?}", files);
            let font_filenames: Vec<(WeeResult<String>, String)> = files
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

            load_fonts(font_files, fonts, font_filenames, ttf_context)
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
    select_sprite_type(&mut object.sprite, ui, asset_files, images, filename);

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

    choose_angle(&mut object.angle, ui);

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
