#![allow(unused, non_snake_case, non_upper_case_globals)]

mod lua;

use std::{borrow::Borrow, ops::Deref, path::{Path, PathBuf}, sync::{Arc, OnceLock, RwLock}, time::Duration};

pub use anyhow::Result as AResult;
use bevy::{asset::{io::AssetSourceEvent, AssetLoader, AsyncReadExt, LoadedFolder}, color::palettes::css, core_pipeline::Skybox, input::mouse::{MouseMotion, MouseWheel}, math::{dvec2, vec2, vec3}, pbr::DirectionalLightShadowMap, prelude::*, render::{camera::RenderTarget, mesh::PrimitiveTopology, render_asset::RenderAssetUsages, render_resource::{Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension}, texture::BevyDefault}, tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task}, utils::{HashMap, HashSet}, window::{PrimaryWindow, WindowResolution}, winit::WinitSettings};
use bevy_egui::{egui::{self, load::SizedTexture, ImageSource, TextureId}, EguiContexts, EguiPlugin};
use crossbeam_channel::Receiver;
use notify::{event::{CreateKind, ModifyKind, RemoveKind, RenameMode}, EventKind, RecursiveMode, Watcher};

const skyboxTexture: &'static str = "skybox/clouds.jpg";

fn main() -> AppExit {
	let mut app = App::new();

	app.add_plugins(DefaultPlugins.set(WindowPlugin {
		primary_window: Some(Window {
			title: "noisebench".into(),
			resolution: WindowResolution::new(1280.0, 720.0),
			resizable: true,
			position: WindowPosition::Centered(MonitorSelection::Primary),
			..default()
		}),
		..default()
	}));
	app.add_plugins(EguiPlugin);

	app.add_event::<NoiseGenRequest>();

	app.add_systems(Startup, setup);
	app.add_systems(PreUpdate, update_viewport_size);
	app.add_systems(Update, (
		close_on_esc,
		axes_gizmo,
		setup_cubemap,
		wef,
		main_ui,
		camera_controller_2d,
		camera_controller_3d,
		scripts_changed,
		generate_noise,
		update_noise_outputs,
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

	let (sender, receiver) = crossbeam_channel::unbounded();
	let mut watcher = notify::recommended_watcher(move |res| {
		match res {
			Ok(event) => { sender.send(event); },
			Err(err) => error!("filesystem watcher error: {err:?}"),
		}
	}).unwrap();
	watcher.watch(Path::new("assets/scripts"), RecursiveMode::Recursive).unwrap();
	let mut scripts = HashMap::new();
	for file in std::fs::read_dir("assets/scripts").unwrap() {
		let file = file.unwrap();
		let ty = file.file_type().unwrap();
		if !ty.is_file() { continue; }

		let path = file.path();
		let path = InternedPath::new(path);
		let contents = std::fs::read_to_string(&path.path).unwrap();
		scripts.insert(path, contents);
	}
	app.insert_resource(LuaScripts {
		channel: receiver,
		scripts,
		selected: None,
	});

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

fn axes_gizmo(mut gizmos: Gizmos) {
	gizmos.line(Vec3::ZERO, Vec3::X * 5.0, css::RED);
	gizmos.line(Vec3::ZERO, Vec3::Y * 5.0, css::GREEN);
	gizmos.line(Vec3::ZERO, Vec3::Z * 5.0, css::BLUE);
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

#[derive(Resource)]
struct NoiseImage(Handle<Image>);

#[derive(Resource)]
struct LuaScripts {
	channel: Receiver<notify::Event>,
	scripts: HashMap<InternedPath, String>,
	selected: Option<InternedPath>,
}

fn setup(
	mut cmd: Commands,
	mut eguiCtx: EguiContexts,
	assets: Res<AssetServer>,
	mut images: ResMut<Assets<Image>>,
	mut materials: ResMut<Assets<StandardMaterial>>,
	mut meshes: ResMut<Assets<Mesh>>,
	mut viewport2d: ResMut<Viewport2D>,
	mut viewport3d: ResMut<Viewport3D>,
	mut noiseGenRequests: EventWriter<NoiseGenRequest>,
) {
	let noiseImage = Image::new_fill(
		Extent3d { width: 256, height: 256, depth_or_array_layers: 1 },
		TextureDimension::D2,
		bytemuck::cast_slice(&[0f32; 4]),
		TextureFormat::Rgba32Float,
		default(),
	);
	let noiseImage = images.add(noiseImage);
	cmd.insert_resource(NoiseImage(noiseImage.clone()));

	let camera2d = cmd.spawn(Camera2dBundle {
		camera: Camera {
			target: RenderTarget::Image(viewport2d.bevyImage.clone()),
			..default()
		},
		..default()
	}).id();
	cmd.spawn((TargetCamera(camera2d), SpriteBundle {
		texture: noiseImage,
		..default()
	}));

	cmd.spawn((Camera3dBundle {
		camera: Camera {
			target: RenderTarget::Image(viewport3d.bevyImage.clone()),
			..default()
		},
		transform: Transform::from_xyz(-2.5, 2.5, -2.5).looking_at(Vec3::ZERO, Vec3::Y),
		..default()
	},
	Skybox {
		image: assets.load(skyboxTexture),
		brightness: 1000.0,
	}));
	cmd.insert_resource(CameraControllerSettings {
		initialAngles: vec2(225.0, -35.0),
		baseSpeed: 10.0,
		..default()
	});
	cmd.spawn(DirectionalLightBundle {
		transform: Transform::IDENTITY.looking_at(-vec3(-0.8035929, 0.39474383, -0.44543877), Vec3::Y),
		directional_light: DirectionalLight {
			shadows_enabled: true,
			..default()
		},
		..default()
	});
	cmd.insert_resource(DirectionalLightShadowMap {
		size: 8192,
	});

	let mesh = meshes.add(Cuboid::from_size(Vec3::ONE));
	let material = materials.add(StandardMaterial {
		base_color_texture: Some(assets.load("test.png")),
		..default()
	});
	cmd.spawn((DbgCube, PbrBundle {
		mesh,
		material,
		transform: Transform::from_xyz(0.0, 0.5, 0.0),
		..default()
	}));

	// water
	let mesh = meshes.add(Rectangle::new(2f32.powi(14), 2f32.powi(14)));
	let material = materials.add(StandardMaterial {
		base_color: Color::srgba_u8(0x11, 0x7f, 0xd5, 0x7f),
		alpha_mode: AlphaMode::Blend,
		..default()
	});
	cmd.spawn(PbrBundle {
		mesh,
		material,
		transform: Transform::from_xyz(0.0, 0.5, 0.0).looking_to(Vec3::NEG_Y, Vec3::Z),
		..default()
	});

	viewport2d.eguiImage = eguiCtx.add_image(viewport2d.bevyImage.clone_weak());
	viewport3d.eguiImage = eguiCtx.add_image(viewport3d.bevyImage.clone_weak());
}

fn setup_cubemap(
	assets: Res<AssetServer>,
	mut images: ResMut<Assets<Image>>,
	mut done: Local<bool>,
) {
	if *done {
		return;
	}

	let Some(handle) = assets.get_handle(skyboxTexture) else { return };
	if !assets.is_loaded_with_dependencies(handle.id()) {
		return;
	}

	let image = images.get_mut(handle.id()).unwrap();
	image.reinterpret_stacked_2d_as_array(image.height() / image.width());
	image.texture_view_descriptor = Some(TextureViewDescriptor {
		dimension: Some(TextureViewDimension::Cube),
		..default()
	});
	*done = true;
}

#[derive(Component)]
struct DbgCube;

fn wef(
	mut query: Query<&mut Transform, With<DbgCube>>,
	time: Res<Time>,
) {
	let mut t = query.single_mut();
	let (x, y) = time.elapsed_seconds().sin_cos();
	let pos = vec3(x, 0.5, y);
	t.look_at(pos, Vec3::Y);
}

fn main_ui(
	mut eguiCtx: EguiContexts,
	mut selectedTab: ResMut<SelectedTab>,
	mut viewportSize: ResMut<ViewportSize>,
	viewport2d: Res<Viewport2D>,
	viewport3d: Res<Viewport3D>,
	images: Res<Assets<Image>>,
	mut luaScripts: ResMut<LuaScripts>,
	mut noiseGenRequests: EventWriter<NoiseGenRequest>,
) {
	let eguiCtx = eguiCtx.ctx_mut();
	egui::TopBottomPanel::top("toolbar").show(eguiCtx, |ui| {
		ui.horizontal(|ui| {
			ui.selectable_value(&mut selectedTab.0, Tab::D2, "2D");
			ui.selectable_value(&mut selectedTab.0, Tab::D3, "3D");

			ui.add_space(50.0);

			let LuaScripts { scripts, selected, .. } = &mut *luaScripts;
			egui::ComboBox::from_id_source("script")
				.selected_text(match selected {
					None => "",
					Some(path) => &path.display,
				})
				.show_ui(ui, |ui| {
					let current = selected.clone();
					for (i, path) in scripts.keys().enumerate() {
						ui.selectable_value(selected, Some(path.clone()), &path.display);
					}
					if *selected != current {
						noiseGenRequests.send(NoiseGenRequest);
					}
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
	mut lastSize: Local<UVec2>,
) {
	if viewportSize.0 == *lastSize {
		return;
	}
	*lastSize = viewportSize.0;

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

fn camera_controller_2d(
	mut camera: Query<&mut Transform, With<Camera2d>>,
	time: Res<Time>,
	keyboard: Res<ButtonInput<KeyCode>>,
	mouseButtons: Res<ButtonInput<MouseButton>>,
	selectedTab: Res<SelectedTab>,
	mut mouseMotion: EventReader<MouseMotion>,
	mut mouseScroll: EventReader<MouseWheel>,
	mut zoom: Local<f32>,
	mut init: Local<bool>,
) {
	if selectedTab.0 != Tab::D2 {
		return;
	}

	if !*init {
		*init = true;
		*zoom = 1.0;
	}

	let mut cameraTransform = camera.single_mut();

	if keyboard.just_pressed(KeyCode::Space) {
		cameraTransform.translation = Vec3::ZERO;
	}

	if mouseButtons.pressed(MouseButton::Left) {
		let mut motion = Vec2::ZERO;
		for event in mouseMotion.read() {
			motion += event.delta;
		}
		motion.x *= -1.0;
		motion *= *zoom;
		cameraTransform.translation += Vec3::from((motion, 0.0));
	} else {
		mouseMotion.clear();
	}

	let mut zoomDelta = 0.0;
	for event in mouseScroll.read() {
		zoomDelta -= event.y;
	}
	*zoom += zoomDelta * 0.1;
	*zoom = zoom.clamp(0.1, 4.0);
	if zoomDelta != 0.0 {
		cameraTransform.scale = Vec3::splat(*zoom);
	}
}

#[derive(Resource, Clone, Copy, Debug)]
struct CameraControllerSettings {
	pub initialAngles: Vec2,
	pub mouseSensitivity: f32,
	pub baseSpeed: f32,
}

impl Default for CameraControllerSettings {
	fn default() -> Self {
		Self {
			initialAngles: default(),
			mouseSensitivity: 0.25,
			baseSpeed: 1.0,
		}
	}
}

fn camera_controller_3d(
	mut camera: Query<&mut Transform, With<Camera3d>>,
	time: Res<Time>,
	keyboard: Res<ButtonInput<KeyCode>>,
	mouseButtons: Res<ButtonInput<MouseButton>>,
	settings: Option<Res<CameraControllerSettings>>,
	selectedTab: Res<SelectedTab>,
	mut mouseMotion: EventReader<MouseMotion>,
	mut angles: Local<Vec2>,
	mut initialized: Local<bool>,
) {
	if selectedTab.0 != Tab::D3 {
		return;
	}

	let defaultSettings;
	let settings = match settings {
		Some(ref res) => res.deref(),
		None => {
			defaultSettings = default();
			&defaultSettings
		}
	};

	if !*initialized {
		*initialized = true;
		*angles = settings.initialAngles;
	}

	if mouseButtons.pressed(MouseButton::Left) {
		let mut motion = Vec2::ZERO;
		for ev in mouseMotion.read() {
			motion += -ev.delta * settings.mouseSensitivity;
		}
		*angles += motion;
		angles.y = angles.y.clamp(-89.9, 89.9);
	} else {
		mouseMotion.clear();
	}

	let mut velocity = Vec3::ZERO;
	if keyboard.pressed(KeyCode::KeyW) {
		velocity.z += 1.0;
	}
	if keyboard.pressed(KeyCode::KeyS) {
		velocity.z -= 1.0;
	}
	if keyboard.pressed(KeyCode::KeyD) {
		velocity.x += 1.0;
	}
	if keyboard.pressed(KeyCode::KeyA) {
		velocity.x -= 1.0;
	}
	if keyboard.pressed(KeyCode::KeyQ) {
		velocity.y += 1.0;
	}
	if keyboard.pressed(KeyCode::KeyZ) {
		velocity.y -= 1.0;
	}

	let mut transform = camera.single_mut();
	transform.rotation =
		Quat::from_rotation_y(angles.x.to_radians()) * Quat::from_rotation_x(angles.y.to_radians());
	let forward = transform
		.forward()
		.reject_from_normalized(Vec3::Y)
		.normalize();
	let right = transform
		.right()
		.reject_from_normalized(Vec3::Y)
		.normalize();
	let up = Vec3::Y;
	let speed = settings.baseSpeed * if keyboard.pressed(KeyCode::ShiftLeft) {
		2.0
	} else if keyboard.pressed(KeyCode::AltLeft) {
		4.0
	} else if keyboard.pressed(KeyCode::ControlLeft) {
		0.5
	} else {
		1.0
	};
	transform.translation += (forward * velocity.z + right * velocity.x + up * velocity.y)
		.normalize_or_zero() *
		speed * time.delta_seconds();
}

#[derive(Event)]
struct NoiseGenRequest;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct InternedPath(Arc<InternedPathInner>);

#[derive(Debug, PartialEq, Eq, Hash)]
struct InternedPathInner {
	pub path: PathBuf,
	pub display: String,
}

impl InternedPath {
	pub fn new(mut path: PathBuf) -> Self {
		static internedPaths: OnceLock<RwLock<HashSet<InternedPath>>> = OnceLock::new();
		let interned = internedPaths.get_or_init(|| RwLock::new(HashSet::new()));

		path = path.canonicalize().unwrap();

		let read = interned.read().unwrap();
		for ipath in read.iter() {
			if ipath.path == path {
				return ipath.clone();
			}
		}

		drop(read);
		let mut write = interned.write().unwrap();
		let display = path.file_name().unwrap().to_str().unwrap();
		let display = format!("{display}");
		let ipath = Self(Arc::new(InternedPathInner {
			path,
			display
		}));
		write.insert(ipath.clone());
		ipath
	}
}

impl Deref for InternedPath {
	type Target = InternedPathInner;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl From<PathBuf> for InternedPath {
	fn from(path: PathBuf) -> Self {
		Self::new(path)
	}
}

impl Borrow<Path> for InternedPath {
	fn borrow(&self) -> &Path {
		&self.path
	}
}

impl Borrow<PathBuf> for InternedPath {
	fn borrow(&self) -> &PathBuf {
		&self.path
	}
}

fn scripts_changed(
	mut luaScripts: ResMut<LuaScripts>,
	mut noiseGenRequests: EventWriter<NoiseGenRequest>,
) {
	fn read_script(path: &Path) -> String {
		std::fs::read_to_string(path).unwrap()
	}

	let LuaScripts { channel, scripts, selected } = &mut *luaScripts;
	while let Ok(ev) = channel.recv_timeout(Duration::ZERO) {
		info!("got event {ev:?}");
		match ev.kind {
			EventKind::Create(CreateKind::File) => {
				let path = &ev.paths[0];
				scripts.insert(InternedPath::new(path.clone()), read_script(path));
			},
			EventKind::Remove(RemoveKind::File) => {
				let path = &ev.paths[0];
				scripts.remove(path);
			},
			EventKind::Modify(ModifyKind::Data(_)) => {
				let path = ev.paths[0].canonicalize().unwrap();
				for (ipath, contents) in scripts.iter_mut() {
					if ipath.path == path {
						*contents = read_script(&path);
					}
				}
				if selected.as_ref().map(Borrow::borrow) == Some(&path) {
					noiseGenRequests.send(NoiseGenRequest);
				}
			},
			EventKind::Modify(ModifyKind::Name(kind)) => {
				match kind {
					RenameMode::To => {
						let path = &ev.paths[0];
						scripts.insert(InternedPath::new(path.clone()), read_script(path));
					},
					RenameMode::From => {
						let path = &ev.paths[0];
						scripts.remove(path);
					},
					RenameMode::Both => {
						let from = &ev.paths[0];
						let to = &ev.paths[0];
					},
					RenameMode::Other | RenameMode::Any => panic!("detected script modification of unknown kind"),
				}
			},
			EventKind::Modify(ModifyKind::Any) => {
				let path = ev.paths[0].canonicalize().unwrap();
				for (ipath, contents) in scripts.iter_mut() {
					if ipath.path == path {
						*contents = read_script(&path);
					}
				}
				if selected.as_ref().map(Borrow::borrow) == Some(&path) {
					noiseGenRequests.send(NoiseGenRequest);
				}
			},
			_ => {},
		}
	}
}

struct NoiseOutput {
	diameter: usize,
	samples: Vec<f32>,
}

impl NoiseOutput {
	pub fn new(diameter: usize) -> Self {
		Self {
			diameter,
			samples: vec![0.0; diameter.pow(2)],
		}
	}

	pub fn rows(&mut self) -> impl '_ + Iterator<Item = (usize, &mut [f32])> {
		self
			.samples
			.chunks_exact_mut(self.diameter)
			.enumerate()
	}
}

#[derive(Component)]
struct NoiseGenTask(Task<NoiseOutput>);

fn generate_noise(
	mut cmd: Commands,
	existingRequests: Query<(Entity, &NoiseGenTask)>,
	luaScripts: Res<LuaScripts>,
	mut noiseGenRequests: EventReader<NoiseGenRequest>,
) {
	let mut requested = false;
	for ev in noiseGenRequests.read() {
		if requested {
			panic!("multiple noise generation requests in one frame");
		}
		requested = true;
		break;
	}
	if !requested { return; }
	info!("noise gen requested");

	for (ent, task) in existingRequests.iter() {
		// FIXME: despawn should also cancel but would be nice to be explicit
		// task.0.cancel();
 		cmd.entity(ent).despawn();
	}

	let code = {
		let selected = luaScripts.selected.as_ref().unwrap();
		luaScripts.scripts.get(selected).unwrap().clone()
	};

	let threadPool = AsyncComputeTaskPool::get();
	let task = threadPool.spawn(async move {
		let mut img = NoiseOutput::new(256);

		let ast = match lua::construct_noisegen(&code) {
			Ok(ast) => ast,
			Err(err) => {
				eprintln!("Lua error: {err:#?}");
				return img;
			},
		};

		let diameter = img.diameter;
		threadPool.scope(|scope| {
			img.rows().for_each(|(y, chunk)| {
				let ast = ast.clone();
				scope.spawn(async move {
					for (x, v) in chunk.into_iter().enumerate() {
						let y = y as f64 / (diameter - 1) as f64;
						let x = x as f64 / (diameter - 1) as f64;
						*v = ast.eval(dvec2(x, y));
					}
				});
			});
		});
		img
	});
	cmd.spawn(NoiseGenTask(task));
}

fn update_noise_outputs(
	mut cmd: Commands,
	mut task: Query<(Entity, &mut NoiseGenTask)>,
	mut images: ResMut<Assets<Image>>,
	noiseImage: Res<NoiseImage>,
) {
	let Ok((taskEnt, mut task)) = task.get_single_mut() else { return };
	let Some(noiseOutput) = block_on(future::poll_once(&mut task.0)) else { return };
	cmd.entity(taskEnt).despawn();
	info!("noise gen done");

	let noiseImage = images.get_mut(&noiseImage.0).unwrap();
	let diameter = noiseOutput.diameter as _;
	if diameter != noiseImage.size().x {
		noiseImage.resize(Extent3d { width: diameter, height: diameter, depth_or_array_layers: 1 });
	}
	let data: &mut [[f32; 4]] = bytemuck::cast_slice_mut(&mut noiseImage.data);
	data.iter_mut().enumerate().for_each(|(i, pixel)| {
		let v = noiseOutput.samples[i];
		let v = (v + 1.0) / 2.0;
		(&mut pixel[.. 3]).fill(v);
		pixel[3] = 1.0;
	});
}
