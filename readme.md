# Auto Exposure for Bevy HDR

[![crates.io](https://img.shields.io/crates/v/bevy_mod_auto_exposure)](https://crates.io/crates/bevy_mod_auto_exposure)
[![docs.rs](https://docs.rs/bevy_mod_auto_exposure/badge.svg)](https://docs.rs/bevy_mod_auto_exposure)

A [Bevy](https://github.com/bevyengine/bevy) plugin for auto exposure.
Features:
- Setting min/max exposure values for the camera;
- Metering mask to give more weight to certain parts of the image;
- Smooth exposure transition, with speparate settings for brightening and darkening;
- Exposure compensation curves, for example to make dark scenes look actually dark.

## Usage

Setting up auto exposure is easy:

```rust
use bevy::prelude::*;
use bevy_mod_auto_exposure::{AutoExposurePlugin, AutoExposure};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Add the plugin.
        .add_plugins(AutoExposurePlugin)
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3dBundle {
            camera: Camera {
                // make sure you set your camera to hdr
                hdr: true,
                ..default()
            },
            ..default()
        },
        // Add the auto exposure component. You can also use the default option,
        // it's good enough for most cases.
        AutoExposure {
            // Set the exposure range to a bit bigger if your scene has a very
            // high dynamic range.
            min: -16.0,
            max: 16.0,
            // Set the compensation curve to make dark parts look dark on
            // screen.
            compensation_curve: vec![vec2(-16.0, -4.0), vec2(0.0, 0.0)],
            ..default()
        },
    ));
}
```

## Bevy Version Support

I intend to track the latest releases of Bevy.

|  bevy   | bevy_mod_inverse_kinematics |
| ------- | --------------------------- |
|  0.12.1 | 0.1                         |

## Examples

```shell
cargo run --example auto_exposure
```

Note that the example doesn't currently work because of [this issue](https://github.com/bevyengine/bevy/issues/10377)

## Licensing

This project is dual-licensed under either

- MIT License: Available [online](http://opensource.org/licenses/MIT)
- Apache License, Version 2.0: Available [online](http://www.apache.org/licenses/LICENSE-2.0)

at your option.
