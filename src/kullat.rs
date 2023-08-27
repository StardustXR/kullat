use std::{f32::consts::PI, sync::Arc};

use glam::Quat;
use stardust_xr_fusion::{
	client::{Client, FrameInfo, RootHandler},
	core::values::Transform,
	drawable::{Alignment, Text, TextStyle},
	items::camera::CameraItem,
	node::NodeType,
};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::winit_display::WinitDisplayMessage;

pub struct Kullat {
	client: Arc<Client>,
	stardust_rx: Receiver<WinitDisplayMessage>,
	display_tx: Option<Sender<()>>,
	text: Text,
}
impl Kullat {
	pub fn new(client: &Arc<Client>, stardust_rx: Receiver<WinitDisplayMessage>) -> Self {
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
		let mut camera = CameraItem::create(
			client.get_root(),
			Transform::default(),
			glam::f32::Mat4::perspective_rh_gl(70.0f32.to_radians(), aspect_ratio, 0.1, 100.0),
			size,
		)
		.unwrap();

		Kullat {
			client: client.clone(),
			stardust_rx,
			display_tx: None,
			text,
		}
	}

	fn handle_head_pos(&mut self) {
		let hmd = self.client.get_hmd();
		let root = self.client.get_root();
		let text = self.text.alias();
		let transform = hmd.get_position_rotation_scale(&root).unwrap();
		tokio::task::spawn(async move {
			let position = transform.await.unwrap().0;
			text.set_text(format!(
				"{:.1}, {:.1}, {:.1}",
				position.x, position.y, position.z
			))
			.unwrap();
		});
	}
}
impl RootHandler for Kullat {
	fn frame(&mut self, _info: FrameInfo) {
		self.handle_head_pos();
	}
}
