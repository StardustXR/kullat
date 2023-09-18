use color::{rgba, Rgba};
use glam::{f32::Mat4, vec3, Quat, Vec3};
use smithay::{
	backend::allocator::{dmabuf::DmabufFlags, Buffer},
	reexports::winit::event_loop::EventLoopProxy,
};
use stardust_xr_fusion::{
	client::{Client, FrameInfo, RootHandler},
	core::values::{BufferInfo, BufferPlaneInfo, Transform},
	drawable::{Alignment, LinePoint, Lines, Text, TextStyle},
	items::camera::CameraItem,
	node::NodeType,
};
use std::{f32::consts::PI, os::fd::OwnedFd, sync::Arc};
use tokio::sync::mpsc::Receiver;

use crate::winit_display::WinitDisplayMessage;

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
	text: Text,
	_camera: CameraItem,
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

		let size = [1920u32, 1080u32];
		let aspect_ratio = size[0] as f32 / size[1] as f32;
		let proj_matrix = Mat4::perspective_rh_gl(70.0f32.to_radians(), aspect_ratio, 0.1, 1000.0);
		let lines = rectangle(1.0, 1.0)
			.map(|p| proj_matrix.inverse().transform_point3(Vec3::from(p)).into());
		let lines = Lines::create(
			client.get_root(),
			Transform::none(),
			&make_line_points(&lines, 0.01, rgba!(1.0, 0.0, 0.0, 1.0)),
			true,
		)
		.unwrap();
		let _camera =
			CameraItem::create(client.get_root(), Transform::none(), proj_matrix, size).unwrap();

		let camera_alias = _camera.alias();
		let text_alias = text.alias();
		let mut t: u64 = 0;

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

						let modifier = buffer.format().modifier;
						let planes = buffer
							.strides()
							.zip(buffer.offsets())
							.enumerate()
							.map(|(idx, (stride, offset))| BufferPlaneInfo {
								idx: idx as u32,
								offset,
								stride,
								modifier: u64::from(modifier),
							})
							.collect();

						let mut flags = DmabufFlags::empty();
						if buffer.y_inverted() {
							flags.toggle(DmabufFlags::Y_INVERT);
						}

						let buffer_info = BufferInfo {
							height: buffer.height() as u32,
							width: buffer.width() as u32,
							fourcc: buffer.format().code as u32,
							flags: flags.bits(),
							planes: planes,
						};

						let fds: Vec<OwnedFd> = buffer
							.handles()
							.map(|fd| fd.try_clone_to_owned().unwrap())
							.collect();

						let raw_fd = std::os::fd::AsRawFd::as_raw_fd(fds.first().unwrap().clone());
						t = t + 1;
						let _ = text_alias.set_text(format!("Rendering {} : {}", raw_fd, t));

						camera_alias
							.render(buffer_info, fds)
							.unwrap()
							.await
							.unwrap();
						let _ = text_alias.set_text(format!("Rendered {} : {}", raw_fd, t));

						let _ = display_tx.send_event(());
					}
				}
			}
		});

		Kullat {
			client: client.clone(),
			text,
			_camera,
			lines,
		}
	}

	fn handle_head_pos(&mut self) {
		// let hmd = self.client.get_hmd();
		// let root = self.client.get_root();
		// let text = self.text.alias();
		// let transform = hmd.get_position_rotation_scale(&root).unwrap();
		// tokio::task::spawn(async move {
		// 	let position = transform.await.unwrap().0;
		// 	text.set_text(format!(
		// 		"{:.1}, {:.1}, {:.1}",
		// 		position.x, position.y, position.z
		// 	))
		// 	.unwrap();
		// });
	}
}
impl RootHandler for Kullat {
	fn frame(&mut self, info: FrameInfo) {
		self.handle_head_pos();
		let _ = self
			._camera
			.set_rotation(None, Quat::from_rotation_y(info.elapsed as f32));
	}
}
