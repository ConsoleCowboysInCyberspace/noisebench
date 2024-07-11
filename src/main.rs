#![allow(unused, non_snake_case, non_upper_case_globals)]

pub use anyhow::Result as AResult;
use bevy::{color::palettes::css, prelude::*, render::{camera::RenderTarget, render_resource::{Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages}, texture::BevyDefault}, window::{PrimaryWindow, WindowResolution}};
use bevy_egui::{egui::{self, load::SizedTexture, ImageSource, TextureId}, EguiContexts, EguiPlugin};

fn main() -> AppExit {
	let mut app = App::new();

	app.add_plugins(DefaultPlugins.set(WindowPlugin {
		primary_window: Some(Window {
			title: "noisebench".into(),
			resolution: WindowResolution::new(1280.0, 1024.0),
			position: WindowPosition::Centered(MonitorSelection::Primary),
			..default()
		}),
		..default()
	}));
	app.add_plugins(EguiPlugin);

	app.add_systems(Startup, setup);
	app.add_systems(PreUpdate, update_viewport_size);
	app.add_systems(Update, (
		close_on_esc,
		main_ui,
	));

	app.insert_resource(SelectedTab(Tab::D2));
	app.insert_resource(ViewportSize(UVec2::ONE));

	let mut images: Mut<Assets<Image>> = app.world_mut().resource_mut();
	let defaultImage = Image {
		texture_descriptor: TextureDescriptor {
			label: None,
			size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
			dimension: TextureDimension::D2,
			format: TextureFormat::bevy_default(),
			mip_level_count: 1,
			sample_count: 1,
			usage: TextureUsages::TEXTURE_BINDING |
				TextureUsages::RENDER_ATTACHMENT |
				TextureUsages::COPY_DST,
			view_formats: &[],
		},
		..default()
	};
	let (viewport2d, viewport3d) = (
		Viewport2D {
			bevyImage: images.add(defaultImage.clone()),
			eguiImage: TextureId::default(),
		},
		Viewport3D {
			bevyImage: images.add(defaultImage),
			eguiImage: TextureId::default(),
		},
	);
	app.insert_resource(viewport2d);
	app.insert_resource(viewport3d);

	app.run()
}

fn close_on_esc(
	keyboard: Res<ButtonInput<KeyCode>>,
	mut exit: EventWriter<AppExit>,
) {
	if keyboard.just_pressed(KeyCode::Escape) {
		exit.send(AppExit::Success);
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
	#[default]
	D2,
	D3,
}

#[derive(Resource)]
struct SelectedTab(pub Tab);

#[derive(Resource)]
struct ViewportSize(UVec2);

#[derive(Resource)]
struct Viewport2D {
	bevyImage: Handle<Image>,
	eguiImage: TextureId,
}

#[derive(Resource)]
struct Viewport3D {
	bevyImage: Handle<Image>,
	eguiImage: TextureId,
}

fn setup(
	mut cmd: Commands,
	assets: Res<AssetServer>,
	mut materials: ResMut<Assets<StandardMaterial>>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut viewport2d: ResMut<Viewport2D>,
	mut viewport3d: ResMut<Viewport3D>,
	mut eguiCtx: EguiContexts,
) {
	let camera2d = cmd.spawn(Camera2dBundle {
		camera: Camera {
			target: RenderTarget::Image(viewport2d.bevyImage.clone()),
			..default()
		},
		..default()
	}).id();
	cmd.spawn((TargetCamera(camera2d), ImageBundle {
		image: UiImage::new(assets.load("test.png")),
		..default()
	}));

	cmd.spawn(Camera3dBundle {
		camera: Camera {
			target: RenderTarget::Image(viewport3d.bevyImage.clone()),
			..default()
		},
		transform: Transform::from_xyz(-2.5, 1.0, 2.5).looking_at(Vec3::ZERO, Vec3::Y),
		..default()
	});
	cmd.spawn(DirectionalLightBundle {
		transform: Transform::IDENTITY.looking_at(Vec3::NEG_ONE, Vec3::Y),
		..default()
	});

	let mesh = meshes.add(Cuboid::from_size(Vec3::ONE));
	let material = materials.add(StandardMaterial {
		base_color_texture: Some(assets.load("test.png")),
		..default()
	});
	cmd.spawn(PbrBundle {
		mesh,
		material,
		..default()
	});

	viewport2d.eguiImage = eguiCtx.add_image(viewport2d.bevyImage.clone_weak());
	viewport3d.eguiImage = eguiCtx.add_image(viewport3d.bevyImage.clone_weak());
}

fn main_ui(
	mut eguiCtx: EguiContexts,
	mut selectedTab: ResMut<SelectedTab>,
	mut viewportSize: ResMut<ViewportSize>,
	viewport2d: Res<Viewport2D>,
	viewport3d: Res<Viewport3D>,
	images: Res<Assets<Image>>,
	mut script: Local<&'static str>,
) {
	let eguiCtx = eguiCtx.ctx_mut();
	egui::TopBottomPanel::top("toolbar").show(eguiCtx, |ui| {
		ui.horizontal(|ui| {
			ui.selectable_value(&mut selectedTab.0, Tab::D2, "2D");
			ui.selectable_value(&mut selectedTab.0, Tab::D3, "3D");

			ui.add_space(50.0);

			egui::ComboBox::from_id_source("script")
				.selected_text(*script)
				.show_ui(ui, |ui| {
					ui.selectable_value(&mut *script, "test1.lua", "test1.lua");
					ui.selectable_value(&mut *script, "test2.lua", "test2.lua");
					ui.selectable_value(&mut *script, "test3.lua", "test2.lua");
				});
		});
	});
	egui::CentralPanel::default().show(eguiCtx, |ui| {
		let size = ui.available_size();
		viewportSize.0 = UVec2::from((size.x as _, size.y as _));

		match selectedTab.0 {
			Tab::D2 => {
				let img = ImageSource::Texture(SizedTexture::new(viewport2d.eguiImage, size));
				ui.image(img);
			},
			Tab::D3 => {
				let img = ImageSource::Texture(SizedTexture::new(viewport3d.eguiImage, size));
				ui.image(img);
			},
		}
	});
}

fn update_viewport_size(
	viewportSize: Res<ViewportSize>,
	viewport2d: Res<Viewport2D>,
	viewport3d: Res<Viewport3D>,
	mut images: ResMut<Assets<Image>>,
) {
	let size = Extent3d {
		width: viewportSize.0.x,
		height: viewportSize.0.y,
		depth_or_array_layers: 1,
	};
	let viewport2d = images.get_mut(&viewport2d.bevyImage).unwrap();
	viewport2d.resize(size);
	let viewport3d = images.get_mut(&viewport3d.bevyImage).unwrap();
	viewport3d.resize(size);
}
