use color::{rgba, Rgba};
use glam::Quat;
use glam::{f32::Mat4, vec3, Vec3, Vec4};
use mint::{Quaternion, Vector3};
use smithay::reexports::winit::event_loop::EventLoopProxy;
use stardust_xr_fusion::drawable::{Alignment, LinePoint, Lines, Text, TextStyle};
use stardust_xr_fusion::{
	client::{Client, FrameInfo, RootHandler},
	core::values::Transform,
	items::camera::CameraItem,
	node::NodeType,
};
use std::f32::consts::PI;
use std::sync::Arc;
use tokio::sync::mpsc::Receiver;

use crate::{render::render, winit_display::WinitDisplayMessage};

pub fn rectangle(width: f32, height: f32) -> [Vec3; 4] {
	let half_width = width * 0.5;
	let half_height = height * 0.5;
	let points = [
		[half_width, half_height],
		[-half_width, half_height],
		[-half_width, -half_height],
		[half_width, -half_height],
	];

	core::array::from_fn(|i| {
		let point = points[i];
		vec3(point[0], point[1], 0.0)
	})
}

pub fn make_line_points(vec3s: &[Vec3], thickness: f32, color: Rgba<f32>) -> Vec<LinePoint> {
	vec3s
		.into_iter()
		.map(|point| LinePoint {
			point: (*point).into(),
			thickness,
			color,
		})
		.collect()
}

pub struct Kullat {
	client: Arc<Client>,
	camera: CameraItem,
	text: Text,
	lines: Lines,
}
impl Kullat {
	pub fn new(client: &Arc<Client>, mut stardust_rx: Receiver<WinitDisplayMessage>) -> Self {
		let text = Text::create(
			client.get_root(),
			Transform::from_position_rotation([0.0, 0.0, -1.0], Quat::from_rotation_y(PI)),
			"test",
			TextStyle {
				character_height: 0.05,
				text_align: Alignment::Center.into(),
				..Default::default()
			},
		)
		.unwrap();

		let lines = rectangle(1.0, 1.0);
		let lines = Lines::create(
			client.get_root(),
			Transform::from_scale(Vec3::new(0.3, 0.1875, 1.0)),
			&make_line_points(&lines, 0.01, rgba!(1.0, 1.0, 1.0, 1.0)),
			true,
		)
		.unwrap();

		let proj_matrix = Mat4::orthographic_rh_gl(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
		let camera =
			CameraItem::create(client.get_hmd(), Transform::none(), proj_matrix, [512, 512])
				.unwrap();

		let camera_alias = camera.alias();

		tokio::task::spawn(async move {
			let mut display_tx: Option<EventLoopProxy<()>> = None;
			while let Some(message) = stardust_rx.recv().await {
				match message {
					WinitDisplayMessage::NewDisplay(new_display_tx) => {
						display_tx.replace(new_display_tx);
					}
					WinitDisplayMessage::Render(buffer) => {
						let Some(display_tx) = display_tx.clone() else {
							continue;
						};

						let _ = render(&camera_alias, buffer).await;

						let _ = display_tx.send_event(());
					}
				}
			}
		});

		Kullat {
			client: client.clone(),
			camera,
			text,
			lines,
		}
	}

	fn handle_head_pos(&mut self) {
		let hmd = self.client.get_hmd().alias();
		let camera = self.camera.alias();
		let text = self.text.alias();

		let target = self.lines.get_position_rotation_scale(&hmd).unwrap();
		tokio::task::spawn(async move {
			let target = target.await.unwrap();
			let target_loc: Vec3 = target.0.into();
			let target_rot: Quat = target.1.into();

			let look_at = Mat4::look_at_rh(Vec3::ZERO, target_loc, target_rot * Vec3::Y);
			let camera_rot = Quat::from_mat4(&look_at);
			let _ = camera.set_transform(
				Some(&hmd),
				Transform::from_position_rotation([0.0; 3], camera_rot.inverse()),
			);

			let proj_matrix = projection_mapped_perspective(target, 0.1, 1000.0);
			let _ = camera.set_proj_matrix(proj_matrix);

			text.set_text(format!("{:.1}, {:.1}", target.0.z, target.2.x))
				.unwrap();
		});
	}
}
impl RootHandler for Kullat {
	fn frame(&mut self, _info: FrameInfo) {
		self.handle_head_pos();
	}

	fn save_state(&mut self) -> stardust_xr_fusion::client::ClientState {
		todo!()
	}
}

/// Creates a right-handed projection-mapped perspective projection matrix with [-1,1] depth range.
/// (It's just a normal perspective projection for now)
#[inline]
pub fn projection_mapped_perspective(
	target: (Vector3<f32>, Quaternion<f32>, Vector3<f32>),
	z_near: f32,
	z_far: f32,
) -> Mat4 {
	let target_distance = Vec3::from(target.0).length();
	let target_width = target.2.x;
	let target_height = target.2.y;

	let inv_frust_depth = 1.0 / (z_near - z_far);

	let x = 2.0 * target_distance / target_width;
	let y = 2.0 * target_distance / target_height;
	let z = (z_near + z_far) * inv_frust_depth;
	let w = (2.0 * z_near * z_far) * inv_frust_depth;
	Mat4::from_cols(
		Vec4::new(x, 0.0, 0.0, 0.0),
		Vec4::new(0.0, y, 0.0, 0.0),
		Vec4::new(0.0, 0.0, z, -1.0),
		Vec4::new(0.0, 0.0, w, 0.0),
	)
}
