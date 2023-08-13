mod kullat;
mod winit_display;

use color_eyre::eyre::Result;
use std::thread;

use stardust_xr_fusion::client::Client;

use kullat::Kullat;
use winit_display::WinitDisplay;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
	color_eyre::install()?;
	let (client, stardust_event_loop) = Client::connect_with_async_loop().await?;

	let (stardust_tx, stardust_rx) = tokio::sync::mpsc::channel(2);
	let (display_tx, display_rx) = tokio::sync::mpsc::channel(2);

	let _root = client.wrap_root(Kullat::new(&client, stardust_rx, display_tx));

	// let tokio_handle = Handle::current();

	// let (winit_stop_tx, mut winit_stop_rx) = oneshot::channel::<()>();
	let display_thread = thread::Builder::new().name("display".to_owned()).spawn({
		move || -> Result<()> {
			// let _tokio_guard = tokio_handle.enter();
			let mut display = WinitDisplay::new(display_rx, stardust_tx)?;
			loop {
				display.update();
			}
		}
	})?;

	tokio::select! {
		biased;
		_ = tokio::signal::ctrl_c() => Ok(()),
		e = stardust_event_loop => e?.map_err(|e| e.into()),
	}
}
