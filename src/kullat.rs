use std::{
	f32::consts::PI,
	os::fd::{AsFd, OwnedFd},
	sync::Arc,
};

use glam::Quat;
use smithay::{
	backend::allocator::{dmabuf::DmabufFlags, Buffer},
	reexports::winit::event_loop::EventLoopProxy,
};
use stardust_xr_fusion::{
	client::{Client, FrameInfo, RootHandler},
	core::values::{BufferInfo, BufferPlaneInfo, Transform},
	drawable::{Alignment, Text, TextStyle},
	items::camera::CameraItem,
	node::NodeType,
};
use tokio::sync::mpsc::Receiver;

use crate::winit_display::WinitDisplayMessage;

pub struct Kullat {
	client: Arc<Client>,
	text: Text,
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
		let camera = CameraItem::create(
			client.get_root(),
			Transform::default(),
			glam::f32::Mat4::perspective_rh_gl(70.0f32.to_radians(), aspect_ratio, 0.1, 100.0),
			size,
		)
		.unwrap();

		let camera_alias = camera.alias();
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
						let Some(display_tx) = display_tx.as_ref() else {
							continue;
						};

						let modifier = buffer.format().modifier;
						let planes = (0..buffer.num_planes())
							.zip(buffer.strides())
							.zip(buffer.offsets())
							.map(|((idx, stride), offset)| BufferPlaneInfo {
								idx: idx as u32,
								offset,
								stride,
								modifier: modifier,
							})
							.collect();

						let mut flags = DmabufFlags::empty();
						if buffer.y_inverted() {
							flags.toggle(DmabufFlags::Y_INVERT);
						}

						let buffer_info = BufferInfo {
							size: (buffer.width() as u32, buffer.height() as u32),
							fourcc: buffer.format().code,
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

						let _ = camera_alias.render((buffer_info, fds)).await;
						let _ = text_alias.set_text(format!("Rendered {} : {}", raw_fd, t));

						let _ = display_tx.send_event(());
					}
				}
			}
		});

		Kullat {
			client: client.clone(),
			text,
		}
	}

	fn handle_head_pos(&mut self) {
		let hmd = self.client.get_hmd();
		let root = self.client.get_root();
		let text = self.text.alias();
		let transform = hmd.get_position_rotation_scale(&root).unwrap();
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
	fn frame(&mut self, _info: FrameInfo) {
		self.handle_head_pos();
	}
}
