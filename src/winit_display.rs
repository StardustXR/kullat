use color_eyre::eyre::{bail, eyre, Result};
use smithay::{
	backend::{
		allocator::{
			dmabuf::{Dmabuf, DmabufAllocator},
			vulkan::{self, ImageUsageFlags, VulkanAllocator},
			Allocator, Format, Fourcc,
		},
		egl::context::GlAttributes,
		renderer::{gles::GlesRenderer, Bind, Blit, TextureFilter, Unbind},
		vulkan::{version::Version, Instance, PhysicalDevice},
	},
	reexports::ash::vk::ExtPhysicalDeviceDrmFn,
	utils::Rectangle,
};
use std::{rc::Rc, sync::Arc};
use tokio::sync::mpsc::Sender;
use winit::{
	dpi::LogicalSize,
	event::{Event, WindowEvent},
	event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
	platform::wayland::EventLoopBuilderExtWayland,
	window::WindowBuilder,
};

use tracing::{info, info_span, trace};

use crate::egl::init_egl;

pub enum WinitDisplayMessage {
	NewDisplay(EventLoopProxy<()>),
	Render(Dmabuf),
}

pub fn start(stardust_tx: Sender<WinitDisplayMessage>) -> Result<()> {
	let span = info_span!("backend_winit", window = tracing::field::Empty);
	let _guard = span.enter();
	info!("Initializing a winit backend");

	let window_builder = WindowBuilder::new()
		.with_title("Kullat")
		.with_inner_size(LogicalSize::new(1280.0, 800.0))
		.with_visible(true);

	let attributes = GlAttributes {
		version: (3, 0),
		profile: None,
		debug: cfg!(debug_assertions),
		vsync: true,
	};

	let event_loop = EventLoopBuilder::<()>::with_user_event()
		.with_any_thread(true)
		.build();
	let winit_window = Arc::new(
		window_builder
			.build(&event_loop)
			.map_err(|e| eyre!(e.to_string()))?,
	);

	span.record("window", Into::<u64>::into(winit_window.id()));
	info!("Window created");

	let mut size = winit_window.inner_size();

	let (_display, context, surface, _is_x11) = init_egl(winit_window, attributes)?;

	let egl = Rc::new(surface);
	let mut renderer: GlesRenderer = unsafe { GlesRenderer::new(context)?.into() };

	let mut allocator = match vulkan_allocator() {
		Ok(allocator) => allocator,
		Err(e) => {
			bail!("Failed to create vulkan allocator: {:?}", e)
		}
	};
	info!("Vulkan allocator created"); // TODO: Ask stardust for supported formats
	let selected_format = Format {
		code: Fourcc::Abgr8888,
		modifier: smithay::backend::allocator::Modifier::Linear,
	};

	info!("Buffer format selected: {selected_format:#?}");

	let proxy = event_loop.create_proxy();
	stardust_tx
		.blocking_send(WinitDisplayMessage::NewDisplay(proxy))
		.map_err(|e| eyre!(e.to_string()))?;

	let mut buffers: [Dmabuf; 2] = core::array::from_fn(|_| {
		allocator
			.create_buffer(
				size.width,
				size.height,
				selected_format.code,
				&[selected_format.modifier],
			)
			.map_err(|e| eyre!(e.to_string()))
			.unwrap()
	});
	info!("Buffers created");

	let mut buffer_to_present: Option<usize> = None;
	let mut buffer_to_render: usize = 0;

	stardust_tx
		.blocking_send(WinitDisplayMessage::Render(
			buffers[buffer_to_render].clone(),
		))
		.map_err(|_| eyre!("unable to send render message"))?;

	event_loop.run(move |event, _, control_flow| match event {
		Event::UserEvent(()) => {
			buffer_to_present.replace(buffer_to_render);
			buffer_to_render = (buffer_to_render + 1) % buffers.len();

			stardust_tx
				.blocking_send(WinitDisplayMessage::Render(
					buffers[buffer_to_render].clone(),
				))
				.unwrap();
		}
		Event::MainEventsCleared => {
			let Some(buffer_to_present) = buffer_to_present.take() else {
				return;
			};

			renderer.bind(egl.clone()).unwrap();
			renderer
				.blit_from(
					buffers[buffer_to_present].clone(),
					Rectangle::from_loc_and_size((0, 0), (size.width as i32, size.height as i32)),
					Rectangle::from_loc_and_size((0, 0), (size.width as i32, size.height as i32)),
					TextureFilter::Linear,
				)
				.unwrap();
			egl.swap_buffers(None).unwrap();
			renderer.unbind().unwrap();
		}
		Event::WindowEvent { event, .. } => match event {
			WindowEvent::Resized(new_size) => {
				trace!("Resizing window to {:?}", new_size);

				buffer_to_present = None;
				size = new_size;

				egl.resize(new_size.width as i32, new_size.height as i32, 0, 0);
				for buffer in buffers.iter_mut() {
					*buffer = allocator
						.create_buffer(
							new_size.width,
							new_size.height,
							selected_format.code,
							&[selected_format.modifier],
						)
						.map_err(|e| eyre!(e.to_string()))
						.unwrap();
				}
			}
			WindowEvent::CloseRequested => {
				*control_flow = ControlFlow::Exit;
			}
			_ => (),
		},
		_ => (),
	})
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
