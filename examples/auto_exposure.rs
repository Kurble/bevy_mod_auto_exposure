use bevy::{input::mouse::MouseMotion, math::vec2, prelude::*, window::CursorGrabMode};
use bevy_mod_auto_exposure::{AutoExposure, AutoExposurePlugin};

#[derive(Component)]
struct CameraMarker;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(AutoExposurePlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_camera)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let ball = meshes.add(Sphere::default());

    commands.spawn(PbrBundle {
        mesh: ball.clone(),
        material: materials.add(StandardMaterial {
            base_color: Color::rgb(0.5, 0.5, 1.0),
            ..default()
        }),
        transform: Transform::from_xyz(1.0, 0.0, 0.0),
        ..default()
    });

    commands.spawn(PbrBundle {
        mesh: ball.clone(),
        material: materials.add(StandardMaterial {
            base_color: Color::rgb(0.5, 0.5, 1.0),
            ..default()
        }),
        transform: Transform::from_xyz(-1.0, 0.0, 0.0),
        ..default()
    });

    commands.spawn(PbrBundle {
        mesh: meshes.add(Plane3d::default().mesh().size(5.0, 5.0)),
        material: materials.add(StandardMaterial {
            base_color: Color::rgb(0.2, 0.8, 0.2),
            ..default()
        }),
        transform: Transform::from_xyz(0.0, -1.0, 0.0),
        ..default()
    });

    commands.spawn(PointLightBundle {
        point_light: PointLight {
            intensity: 900000.0,
            range: 100.,
            shadows_enabled: true,
            ..default()
        },
        transform: Transform::from_xyz(8.0, 16.0, 8.0),
        ..default()
    });

    commands.spawn((
        Camera3dBundle {
            camera: Camera {
                hdr: true,
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 6.0),
            ..Default::default()
        },
        AutoExposure {
            min: -16.0,
            max: 16.0,
            compensation_curve: vec![vec2(-16.0, -4.0), vec2(0.0, -2.0), vec2(16.0, 0.0)],
            ..default()
        },
        CameraMarker,
    ));
}

fn rotate_camera(
    mut windows: Query<&mut Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    key: Res<ButtonInput<KeyCode>>,
    mut mouse_motion_events: EventReader<MouseMotion>,
    mut camera: Query<&mut Transform, With<CameraMarker>>,
) {
    let mut window = windows.single_mut();

    if mouse.just_pressed(MouseButton::Left) {
        window.cursor.visible = false;
        window.cursor.grab_mode = CursorGrabMode::Locked;
    }

    if key.just_pressed(KeyCode::Escape) {
        window.cursor.visible = true;
        window.cursor.grab_mode = CursorGrabMode::None;
    }

    for event in mouse_motion_events.read() {
        if !window.cursor.visible {
            for mut camera_transform in camera.iter_mut() {
                camera_transform.rotate(Quat::from_rotation_y(-event.delta.x * 0.005));
                camera_transform.rotate_local(Quat::from_rotation_x(-event.delta.y * 0.005));
            }
        }
    }
}
