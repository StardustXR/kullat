mod egl;
mod kullat;
mod render;
mod winit_display;

use color_eyre::eyre::Result;
use std::thread;
use tracing_subscriber::FmtSubscriber;

use stardust_xr_fusion::client::Client;

use kullat::Kullat;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
	color_eyre::install()?;
	extern crate tracing;
	let subscriber = FmtSubscriber::builder()
		.with_max_level(tracing::Level::DEBUG)
		.finish();
	tracing::subscriber::set_global_default(subscriber).expect("setting tracing default failed");

	let (client, stardust_event_loop) = Client::connect_with_async_loop().await?;

	let (stardust_tx, stardust_rx) =
		tokio::sync::mpsc::channel::<winit_display::WinitDisplayMessage>(2);

	let _root = client.wrap_root(Kullat::new(&client, stardust_rx));

	let _ = thread::Builder::new()
		.name("display".to_owned())
		.spawn(move || -> Result<()> { winit_display::start(stardust_tx) });

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => Ok(()),
		e = stardust_event_loop => e?.map_err(|e| e.into()),
	}
}
