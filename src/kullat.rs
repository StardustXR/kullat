use std::{f32::consts::PI, sync::Arc};

use glam::Quat;
use stardust_xr_fusion::{
	client::{Client, FrameInfo, RootHandler},
	core::values::Transform,
	drawable::{Alignment, Text, TextStyle},
	node::NodeType,
};
use tokio::sync::mpsc::{Receiver, Sender};

pub struct Kullat {
	client: Arc<Client>,
	stardust_rx: Receiver<()>,
	display_tx: Sender<()>,
	text: Text,
}
impl Kullat {
	pub fn new(client: &Arc<Client>, stardust_rx: Receiver<()>, display_tx: Sender<()>) -> Self {
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

		Kullat {
			client: client.clone(),
			stardust_rx,
			display_tx,
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
