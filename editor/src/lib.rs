// TODO: Get the viewports right for full screen editor
// TODO: Add sounds window
// TODO: Unfullscreen after playing in full screen in preview
// TODO: Unfullscreen when coming from menu to editor
// TODO: No linear resizing for some sprites?
// TODO: If there is a sprite loaded then it should be the first option
// TODO: Ignore input after finishing preview
// TODO: Hover over `animation` it plays animation
// TODO: choose_object fails if object has been deleted
// TODO: Stop setting object size to image size when new image added but have option to match image size
// TODO: More pots in higher difficulties
// TODO: Multiple fonts with same size?
// TODO: File new doesn;t ask if want to save
// TODO: EditedGame struct that turns into GameData when saved? Has unified assets and asset_files
// TODO: Show origin debug option
// TODO: Exit fullscreen when in editor
// TODO: Aspect ratio of edited game changes when window size changed
// TODO: Units in time trigger

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
    path::{Path, PathBuf},
    process, str, thread,
    time::{Duration, Instant},
};

use wee::*;
use wee_common::{
    Colour, Flip, Rect, Size, Vec2, WeeResult, AABB, PROJECTION_HEIGHT, PROJECTION_WIDTH,
};

pub const SMALL_BUTTON: [f32; 2] = [100.0, 50.0];
pub const NORMAL_BUTTON: [f32; 2] = [200.0, 50.0];
const WINDOW_SIZE: [f32; 2] = [500.0, 600.0];

pub fn run<'a, 'b>(
    renderer: &mut Renderer,
    event_pump: &mut EventPump,
    imgui: &mut Imgui,
    font_system: &FontSystem<'a, 'b>,
    settings: GameSettings,
) -> WeeResult<()> {
    let mut game = GameData::default();
    let mut assets = Assets::default();
    let mut filename = None;

    let mut windows = Windows {
        main: true,
        objects: true,
        background: false,
        fonts: false,
        music: false,
        demo: false,
    };

    let mut instruction_state = InstructionState {
        mode: InstructionMode::View,
        index: None,
        focus: InstructionFocus::None,
    };
    let mut preview = Preview {
        playback_rate: 1.0,
        difficulty_level: 1,
        last_win_status: None,
        settings,
    };
    let mut object_state = ObjectState {
        index: None,
        rename_object: None,
        new_object: SerialiseObject::default(),
        new_name_buffer: ImString::from("".to_string()),
    };
    let mut animation_editor = AnimationEditor {
        new_sprite: Sprite::Colour(Colour::black()),
        index: 0,
    };

    let mut last_frame = Instant::now();
    let mut editor_status = EditorStatus::None;
    let mut game_position = Vec2::new(172.0, 106.0);
    let mut scale: f32 = 0.8;
    let mut minus_button = ButtonState::Up;
    let mut plus_button = ButtonState::Up;
    let mut show_collision_areas = false;
    let mut new_background = Sprite::Colour(Colour::black());
    let mut show_error = None;

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

        let mut draw_tasks = Vec::new();

        let imgui_frame =
            imgui.prepare_frame(&renderer.window, &event_pump.mouse_state(), &mut last_frame);
        let ui = &imgui_frame.ui;

        if show_error.is_some() {
            ui.open_popup(im_str!("Game Error"));
        }
        ui.popup_modal(im_str!("Game Error")).build(|| {
            if let Some(error) = &show_error {
                ui.text(format!("{}", error));
            }
            if ui.button(im_str!("OK"), NORMAL_BUTTON) {
                show_error = None;
                ui.close_current_popup();
            }
        });

        if windows.demo {
            ui.show_demo_window(&mut windows.demo);
        }

        main_menu_bar_show(
            &mut game,
            &mut assets,
            &mut filename,
            &mut object_state.index,
            &mut instruction_state,
            ui,
            event_pump,
            &font_system.ttf_context,
            &mut editor_status,
            &mut show_collision_areas,
            &mut windows,
        )?;

        let play_game = main_window_show(ui, &mut windows.main, &mut game, &mut preview);

        if play_game {
            let mut game = LoadedGame::with_assets(game.clone(), assets, &font_system)?.start(
                preview.playback_rate,
                preview.difficulty_level,
                settings,
            );
            let completion = game.preview(renderer, event_pump);

            match completion {
                Ok(_) => {
                    preview.last_win_status = Some(game.has_been_won());
                }
                Err(error) => {
                    show_error = Some(error);
                    preview.last_win_status = None;
                }
            }
            assets = game.assets;
            assets.music.stop();
        }

        //ObjectListEditor::from(&mut self).edit();

        objects_window_show(
            ui,
            &mut windows.objects,
            &mut game,
            &mut assets,
            &filename,
            &mut object_state,
            &mut instruction_state,
            &mut animation_editor,
            &mut draw_tasks,
            &font_system.ttf_context,
        );

        if windows.music {
            //standard_window(im_str!("Music"), &mut windows.music).build(ui, ||);
            imgui::Window::new(im_str!("Music"))
                .size(WINDOW_SIZE, imgui::Condition::FirstUseEver)
                .opened(&mut windows.music)
                .menu_bar(true)
                .resizable(false)
                .build(ui, || {
                    fn load_music_file_dialog(
                        game: &mut GameData,
                        music: &mut Option<Music>,
                        filename: &Option<String>,
                    ) -> WeeResult<()> {
                        let audio_path = assets_path(filename, "audio");
                        let response = nfd::open_file_dialog(None, audio_path.to_str())?;

                        if let Response::Okay(file) = response {
                            let path = Path::new(&file);
                            let file_name = path.file_name().unwrap().to_str().unwrap().to_string();

                            let music_load_info = SerialiseMusic {
                                filename: file_name,
                                looped: false,
                            };

                            *music = Music::load(
                                &Some(music_load_info.clone()),
                                path.parent().unwrap().parent().unwrap(), // TODO: Sort this out
                            )?;
                            if music.is_none() {
                                log::error!("Couldn't load {:?}", path);
                            }
                            game.asset_files.music = Some(music_load_info);
                        }

                        Ok(())
                    }
                    if ui.button(im_str!("Set Music"), NORMAL_BUTTON) {
                        load_music_file_dialog(&mut game, &mut assets.music, &filename)
                            .unwrap_or_else(|error| log::error!("{}", error));
                    }

                    if let Some(music) = &game.asset_files.music {
                        if ui.button(&ImString::from(music.filename.clone()), NORMAL_BUTTON) {
                            assets.start_music(1.0, 20.0); // TODO: Volume
                        }
                    }
                });
        }

        if windows.background {
            // TODO: Fix up
            imgui::Window::new(im_str!("Background"))
                .size(WINDOW_SIZE, imgui::Condition::FirstUseEver)
                .opened(&mut windows.background)
                .build(ui, || {
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
                            stack.pop(ui);
                            break;
                        } else {
                            choose_sprite(
                                &mut game.background[i].sprite,
                                ui,
                                &mut game.asset_files.images,
                                &mut assets.images,
                                &filename,
                            );

                            let input_float =
                                |label, value: &mut f32| ui.input_float(label, value).build();
                            input_float(im_str!("Min X"), &mut game.background[i].area.min.x);
                            input_float(im_str!("Min Y"), &mut game.background[i].area.min.y);
                            input_float(im_str!("Max X"), &mut game.background[i].area.max.x);
                            input_float(im_str!("Max Y"), &mut game.background[i].area.max.y);
                        }

                        stack.pop(ui);
                    }
                    if ui.button(im_str!("New Background"), NORMAL_BUTTON) {
                        new_background = Sprite::Colour(Colour::black());
                        ui.open_popup(im_str!("Add a Background"));
                    };
                    ui.popup_modal(im_str!("Add a Background")).build(|| {
                        choose_sprite(
                            &mut new_background,
                            ui,
                            &mut game.asset_files.images,
                            &mut assets.images,
                            &filename,
                        );
                        if ui.button(im_str!("Ok"), NORMAL_BUTTON) {
                            game.background.push(BackgroundPart {
                                sprite: new_background.clone(),
                                area: AABB::new(0.0, 0.0, 1600.0, 900.0),
                            });
                            ui.close_current_popup();
                        }
                        ui.same_line(0.0);
                        if ui.button(im_str!("Cancel"), NORMAL_BUTTON) {
                            ui.close_current_popup();
                        }
                    });
                });
        }

        if windows.fonts {
            imgui::Window::new(im_str!("Fonts"))
                .size(WINDOW_SIZE, imgui::Condition::FirstUseEver)
                .menu_bar(true)
                .resizable(true)
                .opened(&mut windows.fonts)
                .build(ui, || {
                    if ui.button(im_str!("Add fonts"), NORMAL_BUTTON) {
                        let path = assets_path(&filename, "fonts");
                        choose_font_from_files(
                            &mut game.asset_files.fonts,
                            &mut assets.fonts,
                            path,
                            font_system.ttf_context,
                        );
                    }

                    for (name, font) in game.asset_files.fonts.iter_mut() {
                        ui.text(name);
                        // TODO: Errors if filename not saved!
                        let base_path = assets_path(&filename, "fonts");
                        let path = base_path.join(&font.filename);
                        // TODO: Min Max this
                        if ui.input_float(im_str!("Size"), &mut font.size).build() {
                            *assets.fonts.get_mut(name).unwrap() = font_system
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

            let is_pressed = |scancode| event_pump.keyboard_state().is_scancode_pressed(scancode);

            if is_pressed(Scancode::W) {
                game_position.y += 2.0;
            }
            if is_pressed(Scancode::S) {
                game_position.y -= 2.0;
            }
            if is_pressed(Scancode::A) {
                game_position.x += 2.0;
            }
            if is_pressed(Scancode::D) {
                game_position.x -= 2.0;
            }
        }

        sdlglue::clear_screen(Colour::dull_grey());

        let scene_location = SceneLocation {
            position: game_position,
            scale,
        };

        renderer.clear_editor_screen(scene_location);

        renderer.draw_editor_background(&game.background, &assets.images, scene_location)?;

        renderer.draw_editor_objects(
            &game.objects,
            &assets.images,
            scene_location,
            show_collision_areas,
        )?;

        renderer.draw_tasks(&mut draw_tasks, scene_location);

        imgui_frame.render(&renderer.window);
        renderer.present();

        thread::sleep(Duration::from_millis(10));

        if let EditorStatus::ReturnToMenu = editor_status {
            break 'editor_running;
        }
    }

    Ok(())
}

#[derive(Copy, Clone, Debug)]
struct SceneLocation {
    position: Vec2,
    scale: f32,
}
trait RenderEditor {
    fn clear_editor_screen(&self, location: SceneLocation);

    fn draw_editor_background(
        &self,
        background: &[BackgroundPart],
        images: &Images,
        location: SceneLocation,
    ) -> WeeResult<()>;

    fn draw_editor_objects(
        &self,
        objects: &Vec<SerialiseObject>,
        images: &Images,
        location: SceneLocation,
        show_collision_areas: bool,
    ) -> WeeResult<()>;

    fn draw_tasks(&self, draw_tasks: &mut Vec<DrawTask>, location: SceneLocation);
}

impl RenderEditor for Renderer {
    fn clear_editor_screen(&self, location: SceneLocation) {
        let dest = Rect::new(
            (PROJECTION_WIDTH / 2.0 + location.position.x) * location.scale,
            (PROJECTION_HEIGHT / 2.0 + location.position.y) * location.scale,
            PROJECTION_WIDTH * location.scale,
            PROJECTION_HEIGHT * location.scale,
        );
        let model = Model::new(dest, None, 0.0, Flip::default());
        self.fill_rectangle(model, Colour::light_grey());
    }

    fn draw_editor_background(
        &self,
        background: &[BackgroundPart],
        images: &Images,
        location: SceneLocation,
    ) -> WeeResult<()> {
        for part in background.iter() {
            let dest = part
                .area
                .to_rect()
                .move_position(location.position)
                .scale(location.scale);
            match &part.sprite {
                Sprite::Image { name } => {
                    let texture = images.get_image(name)?;

                    self.prepare(&texture).set_dest(dest).draw();
                }
                Sprite::Colour(colour) => {
                    let model = Model::new(dest, None, 0.0, Flip::default());

                    self.fill_rectangle(model, *colour);
                }
            }
        }
        Ok(())
    }

    fn draw_editor_objects(
        &self,
        objects: &Vec<SerialiseObject>,
        images: &Images,
        location: SceneLocation,
        show_collision_areas: bool,
    ) -> WeeResult<()> {
        let mut layers: Vec<u8> = objects.iter().map(|o| o.layer).collect();
        layers.sort();
        layers.dedup();
        layers.reverse();
        for layer in layers.into_iter() {
            for object in objects.iter() {
                if object.layer == layer {
                    let dest = Rect::new(
                        object.position.x,
                        object.position.y,
                        object.size.width,
                        object.size.height,
                    )
                    .move_position(location.position)
                    .scale(location.scale);
                    match &object.sprite {
                        Sprite::Image { name: image_name } => {
                            let texture = images.get_image(image_name)?;

                            self.prepare(&texture)
                                .set_dest(dest)
                                .set_angle(object.angle)
                                .set_origin(Some(object.origin() * location.scale))
                                .flip(object.flip)
                                .draw();
                        }
                        Sprite::Colour(colour) => {
                            let model = Model::new(
                                dest,
                                Some(object.origin() * location.scale),
                                object.angle,
                                object.flip,
                            );

                            self.fill_rectangle(model, *colour);
                        }
                    }

                    if show_collision_areas {
                        // TODO: Tidy up
                        let game_object = object.clone().to_object();
                        let poly = game_object.poly();

                        for i in 0..poly.count() {
                            let v = poly.get_vert(i);
                            let rect = Rect::new(v.x(), v.y(), 10.0, 10.0)
                                .move_position(location.position)
                                .scale(location.scale);
                            let model = Model::new(rect, None, 0.0, Flip::default());
                            self.fill_rectangle(model, Colour::rgba(0.0, 1.0, 0.0, 0.5));
                        }

                        let aabb = game_object.collision_aabb();
                        let mut origin = game_object.origin();
                        if let Some(area) = game_object.collision_area {
                            origin = Vec2::new(origin.x - area.min.x, origin.y - area.min.y);
                        }
                        let rect = aabb
                            .to_rect()
                            .move_position(location.position)
                            .scale(location.scale);
                        let model = Model::new(
                            rect,
                            Some(origin * location.scale),
                            object.angle,
                            Flip::default(),
                        );
                        // TODO: model.move().scale()
                        self.fill_rectangle(model, Colour::rgba(1.0, 0.0, 0.0, 0.5));
                    }
                }
            }
        }
        Ok(())
    }

    fn draw_tasks(&self, draw_tasks: &mut Vec<DrawTask>, location: SceneLocation) {
        for task in draw_tasks {
            match task {
                DrawTask::AABB(aabb) => {
                    let rect = aabb
                        .to_rect()
                        .move_position(location.position)
                        .scale(location.scale);
                    let model = Model::new(rect, None, 0.0, Flip::default());
                    self.fill_rectangle(model, Colour::rgba(0.0, 0.0, 1.0, 0.5));
                }
                DrawTask::Point(point) => {
                    let rect = Rect::new(point.x, point.y, 20.0, 20.0)
                        .move_position(location.position)
                        .scale(location.scale);
                    let model = Model::new(rect, None, 0.0, Flip::default());
                    self.fill_rectangle(model, Colour::rgba(0.5, 0.0, 0.5, 0.5));
                }
            }
        }
    }
}

fn choose_difficulty_level(level: &mut u32, ui: &imgui::Ui) {
    imgui::Slider::new(
        im_str!("Difficulty Level"),
        std::ops::RangeInclusive::new(1, 3),
    )
    .build(ui, level);
}

fn main_window_show(
    ui: &imgui::Ui,
    show_window: &mut bool,
    game: &mut GameData,
    preview: &mut Preview,
) -> bool {
    let mut play_game = false;

    if *show_window {
        imgui::Window::new(im_str!("Main Window"))
            .size([500.0, 200.0], imgui::Condition::FirstUseEver)
            .scroll_bar(true)
            .scrollable(true)
            .resizable(true)
            .opened(show_window)
            .build(ui, || {
                if ui.button(im_str!("Play"), NORMAL_BUTTON) {
                    play_game = true;
                }

                {
                    let mut current_type_position = match &game.game_type {
                        GameType::Minigame => 0,
                        GameType::BossGame => 1,
                        GameType::Other => 2,
                    };
                    let type_names = [im_str!("Minigame"), im_str!("Boss Game"), im_str!("Other")];
                    if imgui::ComboBox::new(im_str!("Game Type")).build_simple_string(
                        ui,
                        &mut current_type_position,
                        &type_names,
                    ) {
                        game.game_type = match current_type_position {
                            0 => GameType::Minigame,
                            1 => GameType::BossGame,
                            2 => GameType::Other,
                            _ => unreachable!(),
                        }
                    }
                }

                if ui.radio_button_bool(im_str!("Published"), game.published) {
                    game.published = !game.published;
                }

                if ui.radio_button_bool(im_str!("Infinite Length"), game.length == Length::Infinite)
                {
                    game.length = if game.length == Length::Infinite {
                        Length::Seconds(4.0)
                    } else {
                        Length::Infinite
                    }
                };

                match &mut game.length {
                    Length::Seconds(seconds) => {
                        imgui::Slider::new(
                            im_str!("Game Length"),
                            std::ops::RangeInclusive::new(2.0, 8.0),
                        )
                        .display_format(im_str!("%.1f"))
                        .build(ui, seconds);
                    }
                    _ => {}
                }

                let mut intro_text = match game.intro_text.to_owned() {
                    Some(text) => ImString::from(text),
                    None => ImString::from("".to_string()),
                };
                ui.input_text(im_str!("Intro Text"), &mut intro_text)
                    .resize_buffer(true)
                    .build();

                game.intro_text = Some(intro_text.to_string());

                imgui::Slider::new(
                    im_str!("Playback rate"),
                    std::ops::RangeInclusive::new(0.1, 4.0),
                )
                .display_format(im_str!("%.01f"))
                .build(ui, &mut preview.playback_rate);

                choose_difficulty_level(&mut preview.difficulty_level, ui);

                if let Some(win_status) = preview.last_win_status {
                    let win_status = if win_status { "Won" } else { "Lost " };
                    ui.text(format!("Last win status: {}", win_status));
                }

                let mut attr = ImString::from(game.attribution.to_owned());
                ui.input_text_multiline(im_str!("Attribution"), &mut attr, [300.0, 300.0])
                    .resize_buffer(true)
                    .build();
                game.attribution = attr.to_str().to_owned();
            });
    }

    play_game
}

fn assets_path(filename: &Option<String>, assets_directory: &str) -> PathBuf {
    match filename {
        Some(filename) => Path::new(filename).parent().unwrap().join(assets_directory),
        None => Path::new("games").to_owned(),
    }
}

fn objects_window_show<'a>(
    ui: &imgui::Ui,
    show_window: &mut bool,
    game: &mut GameData,
    assets: &mut Assets<'a, '_>,
    filename: &Option<String>,
    object_state: &mut ObjectState,
    instruction_state: &mut InstructionState,
    animation_editor: &mut AnimationEditor,
    draw_tasks: &mut Vec<DrawTask>,
    ttf_context: &'a TtfContext,
) {
    if *show_window {
        imgui::Window::new(im_str!("Objects"))
            .size(WINDOW_SIZE, imgui::Condition::FirstUseEver)
            .position([600.0, 60.0], imgui::Condition::FirstUseEver)
            .scroll_bar(true)
            .scrollable(true)
            .resizable(true)
            .opened(show_window)
            .build(ui, || {
                edit_objects_list(
                    ui,
                    &mut game.objects,
                    object_state,
                    instruction_state,
                    &mut assets.images,
                    &mut game.asset_files.images,
                    filename,
                );

                ui.same_line(0.0);

                if let Some(index) = object_state.index {
                    let mut object = game.objects[index].clone();
                    let object_names: Vec<&str> =
                        game.objects.iter().map(|o| o.name.as_ref()).collect();
                    let sprites = game.objects.sprites();

                    edit_object(
                        ui,
                        &mut object,
                        assets,
                        &mut game.asset_files,
                        filename,
                        instruction_state,
                        &sprites,
                        &object_names,
                        game.length,
                        animation_editor,
                        draw_tasks,
                        ttf_context,
                    );

                    game.objects[index] = object;
                }
            });
    }
}

fn choose_collision_area(area: &mut AABB, ui: &imgui::Ui) -> bool {
    ui.input_float(im_str!("Collision Min X"), &mut area.min.x)
        .build()
        || ui
            .input_float(im_str!("Collision Min Y"), &mut area.min.y)
            .build()
        || ui
            .input_float(im_str!("Collision Max X"), &mut area.max.x)
            .build()
        || ui
            .input_float(im_str!("Collision Max Y"), &mut area.max.y)
            .build()
}

fn edit_object<'a>(
    ui: &imgui::Ui,
    object: &mut SerialiseObject,
    assets: &mut Assets<'a, '_>,
    asset_files: &mut AssetFiles,
    filename: &Option<String>,
    instruction_state: &mut InstructionState,
    sprites: &HashMap<&str, &Sprite>,
    object_names: &Vec<&str>,
    game_length: Length,
    animation_editor: &mut AnimationEditor,
    draw_tasks: &mut Vec<DrawTask>,
    ttf_context: &'a TtfContext,
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
                    edit_object_properties(ui, object, asset_files, &mut assets.images, filename);
                });
                tab_item(im_str!("Instructions"), || {
                    edit_instruction_list(
                        ui,
                        &mut object.instructions,
                        instruction_state,
                        assets,
                        asset_files,
                        sprites,
                        object_names,
                        game_length,
                        filename,
                        animation_editor,
                        draw_tasks,
                        ttf_context,
                    );
                });
            });
        });
}

fn edit_object_properties(
    ui: &imgui::Ui,
    object: &mut SerialiseObject,
    asset_files: &mut AssetFiles,
    images: &mut Images,
    filename: &Option<String>,
) {
    choose_sprite(
        &mut object.sprite,
        ui,
        &mut asset_files.images,
        images,
        filename,
    );

    ui.input_float(im_str!("Starting X"), &mut object.position.x)
        .build();
    ui.input_float(im_str!("Starting Y"), &mut object.position.y)
        .build();
    object.size.choose(ui);

    choose_angle(&mut object.angle, ui);

    let choose_origin = |origin: &mut Vec2| {
        ui.input_float(im_str!("Origin X"), &mut origin.x).build()
            || ui.input_float(im_str!("Origin Y"), &mut origin.y).build()
    };

    object.origin = match object.origin {
        Some(mut origin) => {
            choose_origin(&mut origin);
            Some(origin)
        }
        None => {
            let mut origin = object.origin();
            if choose_origin(&mut origin) {
                Some(origin)
            } else {
                None
            }
        }
    };

    object.collision_area = match object.collision_area {
        Some(mut area) => {
            choose_collision_area(&mut area, ui);
            Some(area)
        }
        None => {
            let mut area = wee_common::AABB::new(
                0.0,
                0.0,
                object.size.width as f32,
                object.size.height as f32,
            );
            if choose_collision_area(&mut area, ui) {
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

fn move_back<T>(list: &mut Vec<T>, index: &mut usize) {
    if *index > 0 {
        list.swap(*index, *index - 1);
        *index -= 1;
    }
}
fn move_forward<T>(list: &mut Vec<T>, index: &mut usize) {
    if *index + 1 < list.len() {
        list.swap(*index, *index + 1);
        *index += 1;
    }
}

fn edit_instruction(
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
            .build(ui)
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
            .build(ui)
        {
            *focus = InstructionFocus::Action { index: i };
        }
        match action {
            Action::Random { random_actions } => {
                for action in random_actions.iter() {
                    imgui::Selectable::new(&ImString::new(format!("\t{}", action)))
                        .selected(selected)
                        .build(ui);
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
            InstructionFocus::Trigger { index } => {
                move_back(&mut instruction.triggers, index);
            }
            InstructionFocus::Action { index } => {
                move_back(&mut instruction.actions, index);
            }
            InstructionFocus::None => {}
        }
    }
    ui.same_line(0.0);
    if ui.small_button(im_str!("Down")) {
        match focus {
            InstructionFocus::Trigger { index } => {
                move_forward(&mut instruction.triggers, index);
            }
            InstructionFocus::Action { index } => {
                move_forward(&mut instruction.actions, index);
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
    fn delete<T>(list: &mut Vec<T>, index: &mut usize) {
        if !list.is_empty() {
            list.remove(*index);
            if *index > 0 {
                *index -= 1;
            }
        }
    }
    if ui.small_button(im_str!("Delete")) {
        match focus {
            InstructionFocus::Trigger { index } => {
                delete(&mut instruction.triggers, index);
            }
            InstructionFocus::Action { index } => {
                delete(&mut instruction.actions, index);
            }
            InstructionFocus::None => {}
        }
    }
}

fn edit_instruction_list<'a>(
    ui: &imgui::Ui,
    instructions: &mut Vec<Instruction>,
    instruction_state: &mut InstructionState,
    assets: &mut Assets<'a, '_>,
    asset_files: &mut AssetFiles,
    sprites: &HashMap<&str, &Sprite>,
    object_names: &Vec<&str>,
    game_length: Length,
    filename: &Option<String>,
    animation_editor: &mut AnimationEditor,
    draw_tasks: &mut Vec<DrawTask>,
    ttf_context: &'a TtfContext,
) {
    fn instruction_mut(
        instructions: &mut Vec<Instruction>,
        index: Option<usize>,
    ) -> &mut Instruction {
        let selected_index = index.unwrap_or(0);
        instructions.get_mut(selected_index).unwrap()
    }
    match instruction_state.mode {
        InstructionMode::View => {
            imgui::ChildWindow::new(im_str!("Test"))
                .size([0.0, -ui.frame_height_with_spacing()])
                .horizontal_scrollbar(true)
                .build(ui, || {
                    for (i, instruction) in instructions.iter().enumerate() {
                        ui.tree_node(&im_str!("Instruction {}", i + 1))
                            .flags(imgui::ImGuiTreeNodeFlags::SpanAvailWidth)
                            .bullet(true)
                            .leaf(true)
                            .selected(instruction_state.index == Some(i))
                            .build(|| {
                                if ui.is_item_clicked(imgui::MouseButton::Left) {
                                    instruction_state.index = Some(i);
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

            if ui.small_button(im_str!("Add")) {
                instructions.push(wee::Instruction {
                    triggers: Vec::new(),
                    actions: Vec::new(),
                });
                instruction_state.index = Some(instructions.len() - 1);
                instruction_state.mode = InstructionMode::Edit;
            }
            ui.same_line(0.0);
            if let Some(selected_index) = &mut instruction_state.index {
                if ui.small_button(im_str!("Edit")) {
                    instruction_state.mode = InstructionMode::Edit;
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Delete")) && !instructions.is_empty() {
                    instructions.remove(*selected_index);
                    if *selected_index > 0 {
                        *selected_index -= 1;
                    }
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Up")) && *selected_index > 0 {
                    instructions.swap(*selected_index, *selected_index - 1);
                    *selected_index -= 1;
                }
                ui.same_line(0.0);
                if ui.small_button(im_str!("Down")) && *selected_index + 1 < instructions.len() {
                    instructions.swap(*selected_index, *selected_index + 1);
                    *selected_index += 1;
                }
            }
        }
        InstructionMode::Edit => edit_instruction(
            &mut instruction_mut(instructions, instruction_state.index),
            ui,
            &mut instruction_state.mode,
            &mut instruction_state.focus,
        ),
        InstructionMode::AddTrigger => {
            let instruction = instruction_mut(instructions, instruction_state.index);
            instruction.triggers.push(Trigger::Time(When::Start));
            instruction_state.focus = InstructionFocus::Trigger {
                index: instruction.triggers.len() - 1,
            };
            instruction_state.mode = InstructionMode::EditTrigger;
        }
        InstructionMode::AddAction => {
            let instruction = instruction_mut(instructions, instruction_state.index);
            instruction.actions.push(Action::Win);
            instruction_state.focus = InstructionFocus::Action {
                index: instruction.actions.len() - 1,
            };
            instruction_state.mode = InstructionMode::EditAction;
        }
        InstructionMode::EditTrigger => {
            let instruction = instruction_mut(instructions, instruction_state.index);
            let trigger_index = match instruction_state.focus {
                InstructionFocus::Trigger { index } => index,
                _ => unreachable!(),
            };
            edit_trigger(
                ui,
                &mut instruction.triggers[trigger_index],
                object_names,
                game_length,
                &mut assets.images,
                &mut asset_files.images,
                filename,
                &mut instruction_state.mode,
                draw_tasks,
            );
        }
        InstructionMode::EditAction => {
            let instruction = instruction_mut(instructions, instruction_state.index);
            let action_index = match instruction_state.focus {
                InstructionFocus::Action { index } => index,
                _ => unreachable!(),
            };
            edit_action(
                ui,
                &mut instruction.actions[action_index],
                assets,
                asset_files,
                filename,
                animation_editor,
                object_names,
                draw_tasks,
                ttf_context,
                &mut instruction_state.mode,
            );
        }
    }
}

fn edit_trigger(
    ui: &imgui::Ui,
    trigger: &mut Trigger,
    object_names: &Vec<&str>,
    game_length: Length,
    images: &mut Images,
    image_files: &mut HashMap<String, String>,
    filename: &Option<String>,
    instruction_mode: &mut InstructionMode,
    draw_tasks: &mut Vec<DrawTask>,
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
        Trigger::DifficultyLevel { .. } => 9,
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
        im_str!("Difficulty Level"),
    ];
    if imgui::ComboBox::new(im_str!("Trigger")).build_simple_string(
        ui,
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
                name: first_name(), // TODO: Use current object's name here
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
            9 => Trigger::DifficultyLevel { level: 1 },
            _ => unreachable!(),
        }
    }
    match trigger {
        Trigger::Time(when) => {
            choose_when(when, ui, game_length);
        }
        Trigger::Collision(with) => {
            choose_collision_with(with, ui, object_names, draw_tasks);
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
            choose_property_check(check, ui, images, image_files, filename);
        }
        Trigger::DifficultyLevel { level } => {
            choose_difficulty_level(level, ui);
        }
    }
    if ui.small_button(im_str!("Back")) {
        *instruction_mode = InstructionMode::Edit;
    }
}

fn edit_action<'a>(
    ui: &imgui::Ui,
    action: &mut Action,
    assets: &mut Assets<'a, '_>,
    asset_files: &mut AssetFiles,
    filename: &Option<String>,
    animation_editor: &mut AnimationEditor,
    object_names: &Vec<&str>,
    draw_tasks: &mut Vec<DrawTask>,
    ttf_context: &'a TtfContext,
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
        Action::EndEarly { .. } => 10,
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
        im_str!("End Early"),
    ];
    fn first_or_default<V>(list: &HashMap<String, V>) -> String {
        list.keys().next().cloned().unwrap_or_default()
    }
    if imgui::ComboBox::new(im_str!("Action")).build_simple_string(
        ui,
        &mut current_action_position,
        &action_names,
    ) {
        *action = match current_action_position {
            0 => Action::Win,
            1 => Action::Lose,
            2 => Action::Effect(Effect::None),
            3 => Action::Motion(Motion::Stop),
            4 => Action::PlaySound {
                name: first_or_default(&assets.sounds),
            },
            5 => Action::StopMusic,
            6 => Action::SetProperty(PropertySetter::Angle(AngleSetter::Value(0.0))),
            7 => Action::Animate {
                animation_type: AnimationType::Loop,
                sprites: Vec::new(),
                speed: Speed::Normal,
            },
            8 => Action::DrawText {
                text: "".to_string(),
                font: first_or_default(&assets.fonts),
                colour: Colour::black(),
                resize: TextResize::MatchObject,
                justify: JustifyText::Centre,
            },
            9 => Action::Random {
                random_actions: Vec::new(),
            },
            10 => Action::EndEarly { time_to_end: 0 },
            _ => unreachable!(),
        }
    }

    match action {
        Action::Effect(effect) => {
            effect.choose(ui);
        }
        Action::Motion(motion) => {
            choose_motion(motion, ui, object_names, draw_tasks);
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
            choose_property_setter(
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
            justify,
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
            justify.choose(ui);
        }
        Action::Random { random_actions } => {
            // TODO: Finish this
            ui.text("Actions:");
            fn list_actions(ui: &imgui::Ui, actions: &[Action]) {
                for action in actions.iter() {
                    ui.text(action.to_string());
                }
            }
            list_actions(ui, &random_actions);
            for (i, action) in random_actions.iter_mut().enumerate() {
                let stack = ui.push_id(i as i32);
                if let Action::Random { .. } = action {
                } else {
                    edit_action(
                        ui,
                        action,
                        assets,
                        asset_files,
                        filename,
                        animation_editor,
                        object_names,
                        draw_tasks,
                        ttf_context,
                        instruction_mode,
                    );
                }
                stack.pop(ui);
            }

            if ui.button(im_str!("Add Action"), SMALL_BUTTON) {
                random_actions.push(Action::Win);
            }
        }
        Action::EndEarly { time_to_end } => {
            choose_u32(time_to_end, ui, im_str!("Time to end"));
        }
        _ => {}
    }

    if ui.small_button(im_str!("Back")) {
        *instruction_mode = InstructionMode::Edit;
    }
}

fn edit_objects_list(
    ui: &imgui::Ui,
    objects: &mut Vec<SerialiseObject>,
    object_state: &mut ObjectState,
    instruction_state: &mut InstructionState,
    images: &mut Images,
    image_files: &mut HashMap<String, String>,
    filename: &Option<String>,
) {
    imgui::ChildWindow::new(im_str!("Left"))
        .size([150.0, 0.0])
        .border(true)
        .horizontal_scrollbar(true)
        .build(ui, || {
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
                _Clone {
                    index: usize,
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

            for i in 0..objects.len() {
                if is_being_renamed(&object_state.rename_object, i) {
                    if let Some(rename_details) = &mut object_state.rename_object {
                        if ui
                            .input_text(im_str!("##edit"), &mut rename_details.buffer)
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
                            object_state.rename_object = None;
                        } else if ui.is_item_deactivated() {
                            object_operation = ObjectOperation::Rename {
                                index: rename_details.index,
                                from: rename_details.name.clone(),
                                to: rename_details.buffer.to_string(),
                            };

                            object_state.rename_object = None;
                        }
                    }
                } else {
                    if imgui::Selectable::new(&im_str!("{}", objects[i].name))
                        .selected(object_state.index == Some(i))
                        .build(ui)
                    {
                        object_state.index = Some(i);
                        *instruction_state = InstructionState::default();
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
                        object_state.index = Some(i);
                    }
                }
            }

            if ui.small_button(im_str!("New Object")) {
                object_operation = ObjectOperation::Add;
            }

            ui.popup(im_str!("Edit Object"), || {
                let index = object_state.index.unwrap();
                if ui.button(im_str!("Move up"), NORMAL_BUTTON) {
                    object_operation = ObjectOperation::Move {
                        direction: MoveDirection::Up,
                        index,
                    };
                }

                if ui.button(im_str!("Move down"), NORMAL_BUTTON) {
                    object_operation = ObjectOperation::Move {
                        direction: MoveDirection::Down,
                        index,
                    };
                }

                if ui.button(im_str!("Rename"), NORMAL_BUTTON) {
                    object_state.rename_object = Some(RenameObject {
                        index,
                        name: objects[index].name.clone(),
                        buffer: ImString::from(objects[index].name.clone()),
                    });
                    ui.close_current_popup();
                }

                if ui.button(im_str!("Clone"), NORMAL_BUTTON) {
                    object_operation = ObjectOperation::_Clone { index }
                }

                if ui.button(im_str!("Delete"), NORMAL_BUTTON) {
                    object_operation = ObjectOperation::Delete { index }
                }
            });

            if !ui.is_any_mouse_down() && ui.is_window_focused() {
                if let Some(index) = &mut object_state.index {
                    if up_pressed {
                        let previous_index = (*index).max(1) - 1;
                        objects.swap(*index, previous_index);
                        *index = previous_index;
                    } else if down_pressed {
                        let next_index = (*index + 1).min(objects.len() - 1);
                        objects.swap(*index, next_index);
                        *index = next_index;
                    }
                }
            }

            match object_operation {
                ObjectOperation::Add => {
                    object_state.new_object = SerialiseObject::default();
                    object_state.new_name_buffer = ImString::from("".to_string());
                    ui.open_popup(im_str!("Create Object"));
                }
                ObjectOperation::Rename { index, from, to } => {
                    let duplicate = objects.iter().any(|o| o.name == to);
                    if from == to {
                        ui.close_current_popup();
                    } else if duplicate {
                        // TODO: Add this popup
                        ui.open_popup(im_str!("Duplicate Name"));
                    } else {
                        objects[index].name = to.to_owned();
                        rename_across_objects(objects, &from, &to);
                        ui.close_current_popup();
                    }
                }
                ObjectOperation::_Clone { index } => {
                    object_state.new_object = objects[index].clone();
                    object_state.new_name_buffer = ImString::from("".to_string());
                    ui.open_popup(im_str!("Clone Object"));
                }
                ObjectOperation::Move {
                    direction,
                    mut index,
                } => {
                    match direction {
                        MoveDirection::Up => {
                            move_back(objects, &mut index);
                        }
                        MoveDirection::Down => {
                            move_forward(objects, &mut index);
                        }
                    }
                    object_state.index = Some(index);
                }
                ObjectOperation::Delete { index } => {
                    objects.remove(index);
                    if objects.is_empty() {
                        object_state.index = None;
                    } else {
                        object_state.index = Some(index.max(1) - 1);
                    }
                }
                _ => {}
            }

            ui.popup_modal(im_str!("Create Object")).build(|| {
                let entered = ui
                    .input_text(im_str!("Name"), &mut object_state.new_name_buffer)
                    .resize_buffer(true)
                    .enter_returns_true(true)
                    .build();

                choose_sprite(
                    &mut object_state.new_object.sprite,
                    ui,
                    image_files,
                    images,
                    &filename,
                );
                if let Sprite::Image { name } = &object_state.new_object.sprite {
                    object_state.new_object.size.width = images[name].width as f32;
                    object_state.new_object.size.height = images[name].height as f32;
                }

                if entered || ui.button(im_str!("OK"), NORMAL_BUTTON) {
                    let duplicate = objects
                        .iter()
                        .any(|o| o.name == object_state.new_object.name);
                    if duplicate {
                        ui.open_popup(im_str!("Duplicate Name"));
                    } else {
                        ui.close_current_popup();
                    }
                    object_state.new_object.name =
                        object_state.new_name_buffer.to_str().to_string();
                    objects.push(object_state.new_object.clone());
                    object_state.index = Some(objects.len() - 1);
                    *instruction_state = InstructionState::default();
                    ui.close_current_popup();
                }
                ui.same_line(0.0);
                if ui.button(im_str!("Cancel"), NORMAL_BUTTON) {
                    ui.close_current_popup();
                }
            });

            ui.popup_modal(im_str!("Clone Object")).build(|| {
                let entered = ui
                    .input_text(im_str!("Name"), &mut object_state.new_name_buffer)
                    .resize_buffer(true)
                    .enter_returns_true(true)
                    .build();

                if entered || ui.button(im_str!("OK"), NORMAL_BUTTON) {
                    let duplicate = objects
                        .iter()
                        .any(|o| o.name == object_state.new_object.name);
                    if duplicate {
                        ui.open_popup(im_str!("Duplicate Name"));
                    } else {
                        ui.close_current_popup();
                    }
                    let new_name = object_state.new_name_buffer.to_str().to_string();
                    rename_in_instructions(
                        &mut object_state.new_object.instructions,
                        &object_state.new_object.name,
                        &new_name,
                    );
                    object_state.new_object.name = new_name;
                    objects.push(object_state.new_object.clone());
                    object_state.index = Some(objects.len() - 1);
                    *instruction_state = InstructionState::default();
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
}

fn file_show<'a, 'b>(
    ui: &imgui::Ui,
    game: &mut GameData,
    assets: &mut Assets<'a, 'b>,
    event_pump: &mut EventPump,
    ttf_context: &'a TtfContext,
    filename: &mut Option<String>,
    selected_index: &mut Option<usize>,
    instruction_state: &mut InstructionState,
    editor_status: &mut EditorStatus,
) -> WeeResult<()> {
    let menu = ui.begin_menu(im_str!("File"), true);
    if let Some(menu) = menu {
        file_new(
            ui,
            game,
            assets,
            filename,
            selected_index,
            instruction_state,
        );

        if let Err(error) = file_open(
            ui,
            game,
            assets,
            event_pump,
            ttf_context,
            filename,
            selected_index,
            instruction_state,
        ) {
            menu.end(ui);
            return Err(error);
        }

        file_save(ui, game, filename);

        file_save_as(ui, game, filename);

        *editor_status = file_return_to_menu(ui);

        menu.end(ui);
    }

    Ok(())
}

fn toggle_menu_item(ui: &imgui::Ui, label: &ImStr, opened: &mut bool) {
    let mut toggled = *opened;
    if imgui::MenuItem::new(label).build_with_ref(ui, &mut toggled) {
        *opened = !(*opened);
    }
}

fn main_menu_bar_show<'a, 'b>(
    game: &mut GameData,
    assets: &mut Assets<'a, 'b>,
    filename: &mut Option<String>,
    selected_index: &mut Option<usize>,
    instruction_state: &mut InstructionState,
    ui: &imgui::Ui,
    event_pump: &mut EventPump,
    ttf_context: &'a TtfContext,
    editor_status: &mut EditorStatus,
    show_collision_areas: &mut bool,
    windows: &mut Windows,
) -> WeeResult<()> {
    let menu_bar = ui.begin_main_menu_bar();
    if let Some(bar) = menu_bar {
        if let Err(error) = file_show(
            ui,
            game,
            assets,
            event_pump,
            ttf_context,
            filename,
            selected_index,
            instruction_state,
            editor_status,
        ) {
            bar.end(ui);
            return Err(error);
        }

        view_show(ui, windows);

        let menu = ui.begin_menu(im_str!("Debug"), true);
        if let Some(menu) = menu {
            toggle_menu_item(ui, im_str!("Show Collision Areas"), show_collision_areas);
            menu.end(ui);
        }
        bar.end(ui);
    }

    Ok(())
}

fn file_new(
    ui: &imgui::Ui,
    game: &mut GameData,
    assets: &mut Assets,
    filename: &mut Option<String>,
    selected_object_index: &mut Option<usize>,
    instruction_state: &mut InstructionState,
) {
    if imgui::MenuItem::new(im_str!("New")).build(ui) {
        *game = GameData::default();
        *assets = Assets::default();
        *filename = None;
        *selected_object_index = None;
        *instruction_state = InstructionState::default();
    }
}

fn file_open<'a, 'b>(
    ui: &imgui::Ui,
    game: &mut GameData,
    assets: &mut Assets<'a, 'b>,
    event_pump: &mut EventPump,
    ttf_context: &'a TtfContext,
    filename: &mut Option<String>,
    selected_index: &mut Option<usize>,
    instruction_state: &mut InstructionState,
) -> WeeResult<()> {
    if imgui::MenuItem::new(im_str!("Open")).build(ui) {
        let response = nfd::open_file_dialog(None, Path::new("games").to_str());
        if let Ok(Response::Okay(file_path)) = response {
            for _ in event_pump.poll_iter() {}
            log::info!("File path = {:?}", file_path);
            let new_data = GameData::load(&file_path);
            match new_data {
                Ok(new_data) => {
                    *game = new_data;
                    *assets = Assets::load(&game.asset_files, &file_path, ttf_context)?;
                    *filename = Some(file_path);
                    *selected_index = None;
                    *instruction_state = InstructionState::default();
                }
                Err(error) => {
                    // TODO: More than just logging here. Show error
                    log::error!("Couldn't open file {}", file_path);
                    log::error!("{}", error);
                }
            }
        } else {
            log::error!("Error opening file dialog");
            //ui.open_popup(im_str!("Error Opening File"));
        }
    }

    Ok(())
}

pub fn file_save(ui: &imgui::Ui, game: &GameData, filename: &mut Option<String>) {
    if imgui::MenuItem::new(im_str!("Save")).build(ui) {
        match filename {
            Some(filename) => {
                let s = serde_json::to_string_pretty(game);
                match s {
                    Ok(s) => {
                        std::fs::write(&filename, s).unwrap_or_else(|e| log::error!("{}", e));
                        println!("SAVED! {}", filename);
                    }
                    Err(error) => {
                        log::error!("{}", error);
                    }
                }
            }
            None => save_game_file_as(game, filename),
        }
    }
}

fn save_game_file_as(game: &GameData, filename: &mut Option<String>) {
    let response = nfd::open_save_dialog(None, Path::new("games").to_str());
    match response {
        Ok(Response::Okay(file_path)) => {
            log::info!("File path = {:?}", file_path);
            let s = serde_json::to_string_pretty(game);
            match s {
                Ok(s) => {
                    std::fs::write(&file_path, s).unwrap_or_else(|e| log::error!("{}", e));
                    *filename = Some(file_path);
                }
                Err(error) => {
                    log::error!("{}", error);
                }
            }
        }
        Ok(_) => {}
        Err(error) => {
            log::error!("{}", error);
        }
    }
}

fn file_save_as(ui: &imgui::Ui, game: &GameData, filename: &mut Option<String>) {
    if imgui::MenuItem::new(im_str!("Save As")).build(ui) {
        save_game_file_as(game, filename);
    }
}

pub fn file_return_to_menu(ui: &imgui::Ui) -> EditorStatus {
    if imgui::MenuItem::new(im_str!("Return to Menu")).build(ui) {
        EditorStatus::ReturnToMenu
    } else {
        EditorStatus::None
    }
}

fn view_show(ui: &imgui::Ui, windows: &mut Windows) {
    let toggle = |label, opened: &mut bool| {
        toggle_menu_item(ui, label, opened);
    };

    let menu = ui.begin_menu(im_str!("View"), true);
    if let Some(menu) = menu {
        toggle(im_str!("Main Window"), &mut windows.main);
        toggle(im_str!("Objects"), &mut windows.objects);
        //toggle(im_str!("Sound FX"), &mut windows.sound_fx.opened);
        toggle(im_str!("Background"), &mut windows.background);
        toggle(im_str!("Music"), &mut windows.music);
        toggle(im_str!("Fonts"), &mut windows.fonts);
        toggle(im_str!("Demo Window"), &mut windows.demo);
        menu.end(ui);
    }
}

pub enum EditorStatus {
    ReturnToMenu,
    None,
}

struct RenameObject {
    index: usize,
    name: String,
    buffer: ImString,
}

struct Windows {
    main: bool,
    objects: bool,
    background: bool,
    fonts: bool,
    music: bool,
    demo: bool,
}

#[derive(Copy, Clone, Debug)]
struct Preview {
    playback_rate: f32,
    difficulty_level: u32,
    last_win_status: Option<bool>,
    settings: GameSettings,
}

struct ObjectState {
    index: Option<usize>,
    rename_object: Option<RenameObject>,
    new_object: SerialiseObject,
    new_name_buffer: ImString,
}

struct InstructionState {
    mode: InstructionMode,
    index: Option<usize>,
    focus: InstructionFocus,
}

impl Default for InstructionState {
    fn default() -> InstructionState {
        InstructionState {
            mode: InstructionMode::View,
            index: None,
            focus: InstructionFocus::None,
        }
    }
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

        // TODO: Sort through these closures
        let object_tooltip = |object_name: &str| {
            if ui.is_item_hovered() {
                ui.tooltip(|| match sprites.get(object_name) {
                    Some(Sprite::Image { name: image_name }) => {
                        if let Some(texture) = &images.get(image_name) {
                            let width = texture.width as f32;
                            let height = texture.height as f32;
                            let max_side = width.max(height);
                            let width = width / max_side * 200.0;
                            let height = height / max_side * 200.0;
                            imgui::Image::new(
                                imgui::TextureId::from(texture.id as usize),
                                [width, height],
                            )
                            .build(ui);
                        } else {
                            ui.text_colored(
                                [1.0, 0.0, 0.0, 1.0],
                                format!("Warning: Image `{}` not found", image_name),
                            );
                        }
                    }
                    Some(Sprite::Colour(colour)) => {
                        imgui::ColorButton::new(
                            im_str!("##Colour"),
                            [colour.r, colour.g, colour.b, colour.a],
                        )
                        .size([80.0, 80.0])
                        .build(ui);
                    }
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
                            .build(ui);
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
                            // TODO: Draw task which highlights the area when hovered here
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

enum DrawTask {
    AABB(AABB),
    Point(Vec2),
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

trait Choose {
    // TODO: Should return bool?
    fn choose(&mut self, ui: &imgui::Ui);
}

impl Choose for Vec2 {
    fn choose(&mut self, ui: &imgui::Ui) {
        ui.input_float(im_str!("X"), &mut self.x).build();
        ui.input_float(im_str!("Y"), &mut self.y).build();
    }
}

fn choose_when(when: &mut When, ui: &imgui::Ui, game_length: Length) {
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
        ui,
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
        When::Random { start, end } => match game_length {
            Length::Seconds(seconds) => {
                let mut frames = [*start as i32, *end as i32];
                ui.drag_int2(im_str!("Time"), &mut frames)
                    .min(0)
                    .max((seconds * 60.0) as i32)
                    .build();
                *start = frames[0] as u32;
                *end = frames[1] as u32;
            }
            Length::Infinite => {
                // TODO: Have a look at how this looks
                let mut frames = [*start as i32, *end as i32];
                ui.drag_int2(im_str!("Time"), &mut frames).min(0).build();
                *start = frames[0] as u32;
                *end = frames[1] as u32;
            }
        },
        _ => {}
    }
}

fn find_object(object_names: &Vec<&str>, name: &str) -> usize {
    object_names
        .iter()
        .position(|obj_name| *obj_name == name)
        .unwrap()
}

fn object_keys(object_names: &Vec<&str>) -> Vec<ImString> {
    object_names
        .iter()
        .map(|name| ImString::from(name.to_string()))
        .collect()
}

fn combo_keys<'a>(keys: &Vec<ImString>) -> Vec<&ImString> {
    keys.iter().collect()
}

fn choose_collision_with(
    with: &mut CollisionWith,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    draw_tasks: &mut Vec<DrawTask>,
) {
    const OBJECT: i32 = 0;
    const AREA: i32 = 1;
    let mut collision_type = if let CollisionWith::Object { .. } = with {
        OBJECT
    } else {
        AREA
    };
    let collision_typename = if collision_type == OBJECT {
        "Object".to_string()
    } else {
        "Area".to_string()
    };
    if imgui::Slider::new(
        im_str!("Collision With"),
        std::ops::RangeInclusive::new(0, 1),
    )
    .display_format(&ImString::from(collision_typename))
    .build(ui, &mut collision_type)
    {
        *with = if collision_type == OBJECT {
            CollisionWith::Object {
                name: object_names[0].to_string(),
            }
        } else {
            CollisionWith::Area(AABB::new(0.0, 0.0, 1600.0, 900.0))
        };
    }

    match with {
        CollisionWith::Object { name } => {
            choose_object(name, ui, object_names);
        }
        CollisionWith::Area(area) => {
            choose_collision_area(area, ui);
            draw_tasks.push(DrawTask::AABB(*area));
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
        .build(ui, &mut input_type)
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
            choose_object(name, ui, object_names);
        }
        MouseOver::Area(area) => {
            area.choose(ui);
        }
        _ => {}
    }
}

impl Choose for Size {
    fn choose(&mut self, ui: &imgui::Ui) {
        ui.input_float(im_str!("Width"), &mut self.width).build();
        ui.input_float(im_str!("Height"), &mut self.height).build();
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
            ui,
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
            ui,
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
        .speed(0.1)
        .display_format(im_str!("%.01f%%"))
        .build();
    *percent = chance_percent / 100.0;
}

fn choose_object(object: &mut String, ui: &imgui::Ui, object_names: &Vec<&str>) {
    let mut current_object = find_object(object_names, object);
    let keys = object_keys(object_names);
    if imgui::ComboBox::new(im_str!("Object")).build_simple_string(
        ui,
        &mut current_object,
        &combo_keys(&keys),
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
            ui,
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
    image_files: &mut HashMap<String, String>,
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
                let path = assets_path(&filename, "images");
                let image = choose_image_from_files(image_files, images, path);
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
    image_files: &mut HashMap<String, String>,
    images: &mut Images,
    path: P,
) {
    let first_key = choose_image_from_files(image_files, images, path);
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
    let path = assets_path(filename, "fonts");

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
            ui,
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
    let path = assets_path(&filename, "audio");

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
            ui,
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
                // TODO: Must have problem when two buttons have the same id
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
                // TODO: Does this work with all the buttons having the same id?
                if imgui::ColorButton::new(
                    im_str!("##Colour"),
                    [colour.r, colour.g, colour.b, colour.a],
                )
                .size([108.0, 108.0])
                .build(ui)
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
        choose_sprite(
            &mut editor.new_sprite,
            ui,
            &mut asset_files.images,
            images,
            filename,
        );

        if ui.button(im_str!("OK"), NORMAL_BUTTON) {
            animation.push(editor.new_sprite.clone());
            ui.close_current_popup();
        }
    });
}

fn choose_sprite(
    sprite: &mut Sprite,
    ui: &imgui::Ui,
    image_files: &mut HashMap<String, String>,
    images: &mut Images,
    filename: &Option<String>,
) {
    select_sprite_type(sprite, ui, image_files, images, filename);

    match sprite {
        Sprite::Image { name: image_name } => {
            // TODO: Tidy up
            let path = assets_path(&filename, "images");

            if images.is_empty() {
                if ui.button(im_str!("Add a New Image"), NORMAL_BUTTON) {
                    choose_new_image(sprite, image_files, images, path);
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
                    ui,
                    &mut current_image,
                    &image_names,
                ) {
                    if current_image == image_names.len() - 1 {
                        choose_new_image(sprite, image_files, images, path);
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
    image_files: &mut HashMap<String, String>,
    filename: &Option<String>,
) {
    match property {
        PropertyCheck::Switch(switch_state) => {
            switch_state.choose(ui);
        }
        PropertyCheck::Sprite(sprite) => {
            choose_sprite(sprite, ui, image_files, images, filename);
        }
        _ => {}
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

fn choose_property_setter(
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
        ui,
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
            choose_sprite(sprite, ui, &mut asset_files.images, images, filename);
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
            if ui.radio_button_bool(im_str!("Flipped?"), *flip) {
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
        im_str!("Rotate To Object"),
        im_str!("Rotate To Mouse"),
    ];
    let mut current_angle_position = match setter {
        AngleSetter::Value(_) => 0,
        AngleSetter::Increase(_) => 1,
        AngleSetter::Decrease(_) => 2,
        AngleSetter::Match { .. } => 3,
        AngleSetter::Clamp { .. } => 4,
        AngleSetter::RotateToObject { .. } => 5,
        AngleSetter::RotateToMouse => 6,
    };
    if imgui::ComboBox::new(im_str!("Angle Setter")).build_simple_string(
        ui,
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
            5 => AngleSetter::RotateToObject {
                name: object_names[0].to_string(),
            },
            6 => AngleSetter::RotateToMouse,
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
        AngleSetter::RotateToObject { name } => {
            choose_object(name, ui, object_names);
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
            .build(ui, &mut effect_type)
        {
            *self = if effect_type == 0 {
                Effect::None
            } else {
                Effect::Freeze
            };
        }
    }
}

impl Choose for JustifyText {
    fn choose(&mut self, ui: &imgui::Ui) {
        let mut justify_type = if *self == JustifyText::Left { 0 } else { 1 };
        let justify_typename = if justify_type == 0 {
            "Left".to_string()
        } else {
            "Centre".to_string()
        };
        if imgui::Slider::new(im_str!("Justify Text"), std::ops::RangeInclusive::new(0, 1))
            .display_format(&ImString::from(justify_typename))
            .build(ui, &mut justify_type)
        {
            *self = if justify_type == 0 {
                JustifyText::Left
            } else {
                JustifyText::Centre
            };
        }
    }
}

fn choose_motion(
    motion: &mut Motion,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    draw_tasks: &mut Vec<DrawTask>,
) {
    let mut current_motion_position = match motion {
        Motion::Stop => 0,
        Motion::GoStraight { .. } => 1,
        Motion::JumpTo(_) => 2,
        Motion::Roam { .. } => 3,
        Motion::Swap { .. } => 4,
        Motion::Target { .. } => 5,
        Motion::Accelerate(Acceleration::Continuous { .. }) => 6,
        Motion::Accelerate(Acceleration::SlowDown { .. }) => 7,
    };
    let motion_names = [
        im_str!("Stop"),
        im_str!("Go Straight"),
        im_str!("Jump To"),
        im_str!("Roam"),
        im_str!("Swap"),
        im_str!("Target"),
        im_str!("Accelerate"),
        im_str!("Slow Down"),
    ];
    if imgui::ComboBox::new(im_str!("Motion")).build_simple_string(
        ui,
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
            6 => Motion::Accelerate(Acceleration::Continuous {
                direction: MovementDirection::Angle(Angle::Current),
                speed: Speed::Normal,
            }),
            7 => Motion::Accelerate(Acceleration::SlowDown {
                speed: Speed::Normal,
            }),
            _ => unreachable!(),
        };
    }

    match motion {
        Motion::GoStraight { direction, speed }
        | Motion::Accelerate(Acceleration::Continuous { direction, speed }) => {
            direction.choose(ui);
            speed.choose(ui);
        }
        Motion::Accelerate(Acceleration::SlowDown { speed }) => {
            speed.choose(ui);
        }
        Motion::JumpTo(jump_location) => {
            choose_jump_location(jump_location, ui, object_names, draw_tasks);
        }
        Motion::Roam {
            movement_type,
            area,
            speed,
        } => {
            movement_type.choose(ui);
            area.choose(ui);
            draw_tasks.push(DrawTask::AABB(*area));
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
            ui,
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

fn choose_jump_location(
    jump: &mut JumpLocation,
    ui: &imgui::Ui,
    object_names: &Vec<&str>,
    draw_tasks: &mut Vec<DrawTask>,
) {
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
        ui,
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
            area.choose(ui);

            draw_tasks.push(DrawTask::AABB(*area));
        }
        JumpLocation::Object { name } => {
            choose_object(name, ui, object_names);
        }
        JumpLocation::Point(point) => {
            point.choose(ui);

            draw_tasks.push(DrawTask::Point(*point));
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
            area.choose(ui);

            draw_tasks.push(DrawTask::AABB(*area));
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
            ui,
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
                    ui,
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
        if ui.radio_button_bool(im_str!("Follow"), *self == TargetType::Follow) {
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

trait EnumSetters: Sized {
    fn to_value(&self) -> usize;

    fn from_value(value: usize) -> Self;

    fn component<F: Fn(&mut Self, &ImStr, &[&ImStr])>(&mut self, f: F);

    fn combo(&mut self, ui: &imgui::Ui) {
        self.component(|s, label, types| {
            let mut position = s.to_value();
            if imgui::ComboBox::new(label).build_simple_string(ui, &mut position, types) {
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
    image_files: &mut HashMap<String, String>,
    images: &mut Images,
    image_filenames: Vec<(WeeResult<String>, String)>,
) -> Option<String> {
    let mut first_key = None;
    for (key, path) in image_filenames {
        key.map(|key| {
            Texture::from_file(&path).map(|texture| {
                images.insert(key.clone(), texture);
                let filename = get_filename(&path).unwrap();
                image_files.insert(key.clone(), filename);
                first_key = Some(key.clone());
            })
        })
        .map_err(|error| {
            log::error!("Could not add image with filename {}", path);
            log::error!("{}", error);
        })
        .ok();
    }
    first_key
}

fn load_sounds(
    audio_files: &mut HashMap<String, String>,
    sounds: &mut Sounds,
    sound_filenames: Vec<(WeeResult<String>, String)>,
) -> Option<String> {
    let mut first_key = None;
    for (key, path) in sound_filenames {
        key.map(|key| {
            SoundBuffer::from_file(&path).map(|sound| {
                sounds.insert(key.clone(), sound);
                let filename = get_filename(&path).unwrap();
                audio_files.insert(key.clone(), filename);
                first_key = Some(key.clone());
            })
        })
        .map_err(|error| {
            log::error!("Could not add sound with filename {}", path);
            log::error!("{}", error);
        })
        .ok();
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
    for (key, path) in font_filenames {
        key.map(|key| {
            ttf_context.load_font(&path, 128).map(|font| {
                fonts.insert(key.clone(), font);
                let filename = get_filename(&path).unwrap();
                let load_info = FontLoadInfo {
                    filename,
                    size: 128.0,
                };
                font_files.insert(key.clone(), load_info);
                first_key = Some(key.clone());
            })
        })
        .map_err(|error| {
            log::error!("Could not add font with filename {}", path);
            log::error!("{}", error);
        })
        .ok();
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

fn get_relative_file_path(path: &Path) -> String {
    path.strip_prefix(std::env::current_dir().unwrap().as_path())
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn get_filename<P: AsRef<Path>>(path: P) -> WeeResult<String> {
    path.as_ref()
        .file_name()
        .map(|s| s.to_str())
        .flatten()
        .map(|s| s.to_string())
        .ok_or(format!("Couldn't get the filename from {:?}", path.as_ref()).into())
}

fn get_separate_file_parts(file_path: String) -> (WeeResult<String>, String) {
    let path = std::path::Path::new(&file_path);
    let name = get_main_filename_part(&path);
    let file_path = get_relative_file_path(&path);
    (name, file_path)
}

fn choose_image_from_files<P: AsRef<Path>>(
    image_files: &mut HashMap<String, String>,
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
            let image_filenames: Vec<(WeeResult<String>, String)> =
                files.into_iter().map(get_separate_file_parts).collect();

            load_textures(image_files, images, image_filenames)
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
            let sound_filenames: Vec<(WeeResult<String>, String)> =
                files.into_iter().map(get_separate_file_parts).collect();

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
            let font_filenames: Vec<(WeeResult<String>, String)> =
                files.into_iter().map(get_separate_file_parts).collect();

            load_fonts(font_files, fonts, font_filenames, ttf_context)
        }
        _ => None,
    }
}

pub fn rename_across_objects(objects: &mut Vec<SerialiseObject>, old_name: &str, new_name: &str) {
    for obj in objects.iter_mut() {
        rename_in_instructions(&mut obj.instructions, old_name, new_name);
    }
}

fn rename_in_instructions(instructions: &mut Vec<Instruction>, old_name: &str, new_name: &str) {
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
