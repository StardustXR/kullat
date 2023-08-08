use std::{f32::consts::PI, sync::Arc};
use glam::Quat;
use stardust_xr_fusion::{
    client::{Client, FrameInfo, RootHandler},
    core::values::Transform,
    drawable::{Alignment, Text, TextStyle},
    node::NodeType
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (client, event_loop) = Client::connect_with_async_loop().await.unwrap();

    let _root = client.wrap_root(Kullat::new(&client));

    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => (),
        e = event_loop => e.unwrap().unwrap(),
    };
}

struct Kullat { 
    client: Arc<Client>,
    text: Text
}
impl Kullat {
    fn new(client: &Arc<Client>) -> Self {
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

        Kullat{client: client.clone(), text: text}
    }

    fn handle_head_pos(&mut self){
        let hmd = self.client.get_hmd();
        let root = self.client.get_root();
        let text = self.text.alias();
        let transform = hmd.get_position_rotation_scale(&root).unwrap();
		tokio::task::spawn(async move {
            let position = transform.await.unwrap().0;
            text.set_text(format!("{:.1}, {:.1}, {:.1}", position.x, position.y, position.z)).unwrap();
        });
    }
}
impl RootHandler for Kullat {
    fn frame(&mut self, _info: FrameInfo) {
        self.handle_head_pos();
    }
}
