use bevy::prelude::*;

mod render;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(render::MyRenderPlugin)
        .add_systems(Startup, setup)
        .run()
}

fn setup(mut cmd: Commands, server: Res<AssetServer>) {
    cmd.spawn(Camera2dBundle::default());

    let position = Vec3::new(0., 0., 0.);

    cmd.spawn((
        SpatialBundle::from_transform(Transform::from_translation(position)),
        render::CustomSprite {
            texture: server.load("icon.png"),
        },
    ));
}
