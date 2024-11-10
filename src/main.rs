//! This example cycle through two kinds of isometric maps and display debug informations about Tiled objects.

use avian2d::prelude::*;
use bevy::{color::palettes, prelude::*, window::PrimaryWindow};
use bevy_dexterous_developer::{
    reloadable_main, reloadable_scope, ReloadableApp, ReloadableAppContents,
    ReloadableElementsSetup,
};
use bevy_ecs_tiled::prelude::*;
use bevy_ecs_tilemap::prelude::*;

mod helper;

#[derive(SystemSet, Clone, Copy, Hash, PartialEq, Eq, Debug)]
pub struct SpawnTilemapSet;

reloadable_main!( (initial_plugins) {
    App::new()
        .add_plugins(initial_plugins.initialize::<DefaultPlugins>())
        .add_plugins(TilemapPlugin)
        .add_plugins(TiledMapPlugin)
        .add_plugins(TiledMapDebugPlugin::default())
        .add_plugins(helper::HelperPlugin)
        .add_plugins(PhysicsPlugins::default().with_length_unit(10.0))
        .add_plugins(MyPlugin)
        .add_systems(
            Startup,
            (startup, apply_deferred).chain().in_set(SpawnTilemapSet),
        )
        .insert_resource(Gravity(Vec2::NEG_Y * 1000.0))
        .insert_resource(SpawnedLabels { set: false })
        .init_resource::<CursorPos>()
        .init_resource::<FontHandle>()
        .run();
});

fn startup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2dBundle::default());
    helper::avian::spawn_player(&mut commands, 10., Vec2::new(10., 10.));

    commands.spawn(TiledMapBundle {
        tiled_map: asset_server.load(r#"testmap/untitled.tmx"#),
        render_settings: TilemapRenderSettings {
            // bevy_ecs_tilemap provide the 'y_sort' parameter to
            // sort chunks using their y-axis position during rendering.
            // However, it applies to whole chunks, not individual tile,
            // so we have to force the chunk size to be exactly one tile
            render_chunk_size: UVec2::new(1, 1),
            y_sort: true,
        },
        tiled_settings: TiledMapSettings {
            map_positioning: MapPositioning::LayerOffset,
            ..Default::default()
        },
        ..Default::default()
    });
}

#[derive(Deref, Resource)]
pub struct FontHandle(Handle<Font>);

impl FromWorld for FontHandle {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>();
        Self(asset_server.load(r#"fonts/FiraSans-Bold.ttf"#))
    }
}

#[derive(Resource)]
struct SpawnedLabels {
    set: bool,
}

#[derive(Component)]
struct TileLabel(Entity);

// Generates tile position labels of the form: `(tile_pos.x, tile_pos.y)`
fn spawn_tile_labels(
    mut commands: Commands,
    tilemap_q: Query<(&Transform, &TilemapType, &TilemapGridSize, &TileStorage)>,
    tile_q: Query<&mut TilePos>,
    font_handle: Res<FontHandle>,
    mut spawned_labels: ResMut<SpawnedLabels>,
) {
    if spawned_labels.set {
        return;
    }
    let text_style = TextStyle {
        font: font_handle.clone(),
        font_size: 8.0,
        color: Color::BLACK,
    };
    let text_justify = JustifyText::Center;
    for (map_transform, map_type, grid_size, tilemap_storage) in tilemap_q.iter() {
        for tile_entity in tilemap_storage.iter().flatten() {
            let tile_pos = tile_q.get(*tile_entity).unwrap();
            let tile_center = tile_pos.center_in_world(grid_size, map_type).extend(1.0);
            let mut transform = *map_transform * Transform::from_translation(tile_center);
            transform.translation.z += 1.0;

            let label_entity = commands
                .spawn(Text2dBundle {
                    text: Text::from_section(
                        format!("{}, {}", tile_pos.x, tile_pos.y),
                        text_style.clone(),
                    )
                    .with_justify(text_justify),
                    transform,
                    ..default()
                })
                .id();
            commands
                .entity(*tile_entity)
                .insert(TileLabel(label_entity));

            // Set the spawned labels resource to true so we don't spawn them again.
            spawned_labels.set = true;
        }
    }
}

#[derive(Component)]
struct HighlightedLabel;

#[derive(Resource)]
pub struct CursorPos(Vec2);
impl Default for CursorPos {
    fn default() -> Self {
        // Initialize the cursor pos at some far away place. It will get updated
        // correctly when the cursor moves.
        Self(Vec2::new(-1000.0, -1000.0))
    }
}

// We need to keep the cursor position updated based on any `CursorMoved` events.
pub fn update_cursor_pos(
    camera_q: Query<(&GlobalTransform, &Camera)>,
    mut cursor_moved_events: EventReader<CursorMoved>,
    mut cursor_pos: ResMut<CursorPos>,
) {
    for cursor_moved in cursor_moved_events.read() {
        // To get the mouse's world position, we have to transform its window position by
        // any transforms on the camera. This is done by projecting the cursor position into
        // camera space (world space).
        for (cam_t, cam) in camera_q.iter() {
            if let Some(pos) = cam.viewport_to_world_2d(cam_t, cursor_moved.position) {
                *cursor_pos = CursorPos(pos);
            }
        }
    }
}

// This is where we check which tile the cursor is hovered over.
fn highlight_tile_labels(
    mut commands: Commands,
    cursor_pos: Res<CursorPos>,
    tilemap_q: Query<(
        &TilemapSize,
        &TilemapGridSize,
        &TilemapType,
        &TileStorage,
        &Transform,
    )>,
    highlighted_tiles_q: Query<Entity, With<HighlightedLabel>>,
    tile_label_q: Query<&TileLabel>,
    mut text_q: Query<&mut Text>,
) {
    // Un-highlight any previously highlighted tile labels.
    for highlighted_tile_entity in highlighted_tiles_q.iter() {
        if let Ok(label) = tile_label_q.get(highlighted_tile_entity) {
            if let Ok(mut tile_text) = text_q.get_mut(label.0) {
                for section in tile_text.sections.iter_mut() {
                    section.style.color = Color::BLACK;
                }
                commands
                    .entity(highlighted_tile_entity)
                    .remove::<HighlightedLabel>();
            }
        }
    }

    for (map_size, grid_size, map_type, tile_storage, map_transform) in tilemap_q.iter() {
        // Grab the cursor position from the `Res<CursorPos>`
        let cursor_pos: Vec2 = cursor_pos.0;
        // We need to make sure that the cursor's world position is correct relative to the map
        // due to any map transformation.
        let cursor_in_map_pos: Vec2 = {
            // Extend the cursor_pos vec3 by 0.0 and 1.0
            let cursor_pos = Vec4::from((cursor_pos, 0.0, 1.0));
            let cursor_in_map_pos = map_transform.compute_matrix().inverse() * cursor_pos;
            cursor_in_map_pos.xy()
        };

        // Fix the gap due the dimond grid.
        let cursor_in_map_pos = cursor_in_map_pos - Vec2::new(0.0, grid_size.y / 2.0); // <----------- Here

        // Once we have a world position we can transform it into a possible tile position.
        if let Some(tile_pos) =
            TilePos::from_world_pos(&cursor_in_map_pos, map_size, grid_size, map_type)
        {
            // Highlight the relevant tile's label
            if let Some(tile_entity) = tile_storage.get(&tile_pos) {
                if let Ok(label) = tile_label_q.get(tile_entity) {
                    if let Ok(mut tile_text) = text_q.get_mut(label.0) {
                        for section in tile_text.sections.iter_mut() {
                            section.style.color = palettes::tailwind::BLUE_600.into();
                        }
                        commands.entity(tile_entity).insert(HighlightedLabel);
                    }
                }
            }
        }
    }
}

#[derive(SystemSet, Clone, Copy, Hash, PartialEq, Eq, Debug)]
struct MyPlugin;

impl Plugin for MyPlugin {
    fn build(&self, app: &mut App) {
        app.setup_reloadable_elements::<reloadable>();
    }
}

reloadable_scope!(reloadable(app) {
    app
    .add_systems(Update, spawn_tile_labels)
    .add_systems(Update, (update_cursor_pos, highlight_tile_labels));
});
