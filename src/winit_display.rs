use color_eyre::eyre::{self, bail, eyre, Result};
use smithay::{
	backend::{
		allocator::{
			dmabuf::{Dmabuf, DmabufAllocator},
			vulkan::{self, ImageUsageFlags, VulkanAllocator},
		},
		egl::{EGLContext, EGLDevice, EGLDisplay},
		renderer::{gles::GlesRenderer, Blit, TextureFilter},
		vulkan::{version::Version, Instance, PhysicalDevice},
		winit::{self, WinitEvent, WinitEventLoop, WinitGraphicsBackend},
	},
	reexports::{
		ash::vk::ExtPhysicalDeviceDrmFn,
		winit::{dpi::LogicalSize, window::WindowBuilder},
	},
	utils::Rectangle,
};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::error;

pub struct WinitDisplay {
	pub buffers: Vec<Dmabuf>,
	backend: WinitGraphicsBackend<GlesRenderer>,
	allocator: DmabufAllocator<VulkanAllocator>,
	winit: WinitEventLoop,
}
impl WinitDisplay {
	pub fn new(display_rx: Receiver<()>, stardust_tx: Sender<()>) -> Result<Self> {
		let (mut backend, winit) = winit::init_from_builder::<GlesRenderer>(
			WindowBuilder::new()
				.with_title("Kullat")
				.with_inner_size(LogicalSize::new(1280.0, 800.0))
				.with_visible(true),
		)
		.map_err(|e| eyre!(e.to_string()))?;
		let size = backend.window_size().physical_size;
		let egl_context = backend.renderer().egl_context();
		let texture_formats = egl_context.dmabuf_texture_formats();

		let allocator = match vulkan_allocator() {
			Ok(allocator) => allocator,
			Err(e) => {
				bail!("Failed to create vulkan allocator: {:?}", e)
			}
		};
		error!("Vulkan allocator created");

		//allocator.0.create_buffer_with_usage(size.w, size.h, fourcc;

		Ok(WinitDisplay {
			backend,
			allocator,
			buffers: Vec::new(),
			winit,
		})
	}

	pub fn update(&mut self) {
		self.winit
			.dispatch_new_events(|event| match event {
				WinitEvent::Resized { size, .. } => (),
				WinitEvent::Refresh => {
					self.backend.bind().unwrap();
					self.backend
						.renderer()
						.blit_from(
							self.buffers[0].clone(),
							Rectangle::from_loc_and_size((0, 0), (1, 1)),
							Rectangle::from_loc_and_size((0, 0), (1, 1)),
							TextureFilter::Linear,
						)
						.unwrap();
					self.backend.submit(None).unwrap();
				}
				_ => (),
			})
			.unwrap();
	}
}

fn vulkan_allocator() -> Result<DmabufAllocator<VulkanAllocator>, vulkan::Error> {
	let instance = match Instance::new(Version::VERSION_1_2, None) {
		Ok(instance) => instance,
		Err(_e) => return Err(vulkan::Error::Setup),
	};
	let physical_devices = PhysicalDevice::enumerate(&instance)?;
	let physical_device = physical_devices
		.filter(|phd| phd.has_device_extension(ExtPhysicalDeviceDrmFn::name()))
		.next();
	if physical_device.is_none() {
		return Err(vulkan::Error::Setup);
	}
	let allocator = VulkanAllocator::new(
		&physical_device.unwrap(),
		ImageUsageFlags::COLOR_ATTACHMENT | ImageUsageFlags::SAMPLED,
	)?;
	return Ok(DmabufAllocator(allocator));
}
