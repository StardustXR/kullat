use color_eyre::eyre::{bail, eyre, Result};
use smithay::{
	backend::{
		allocator::{
			dmabuf::{Dmabuf, DmabufAllocator},
			vulkan::{self, ImageUsageFlags, VulkanAllocator},
			Allocator, Format, Fourcc,
		},
		egl::{
			self,
			context::{GlAttributes, PixelFormatRequirements},
			display::EGLDisplay,
			native, EGLContext, EGLSurface,
		},
		renderer::{gles::GlesRenderer, Bind, Blit, TextureFilter, Unbind},
		vulkan::{version::Version, Instance, PhysicalDevice},
	},
	reexports::{
		ash::vk::ExtPhysicalDeviceDrmFn,
		winit::{
			dpi::LogicalSize,
			event::{Event, WindowEvent},
			event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
			platform::{
				wayland::{EventLoopBuilderExtWayland, WindowExtWayland},
				x11::WindowExtX11,
			},
			window::{Window, WindowBuilder},
		},
	},
	utils::Rectangle,
};
use std::{rc::Rc, sync::Arc};
use tokio::sync::mpsc::Sender;
use wayland_egl as wegl;

use tracing::{debug, info, info_span, trace};

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

	let (display, context, surface, _is_x11) = init_egl(winit_window, attributes)?;

	let egl = Rc::new(surface);
	let mut renderer: GlesRenderer = unsafe { GlesRenderer::new(context)?.into() };

	let mut allocator = match vulkan_allocator() {
		Ok(allocator) => allocator,
		Err(e) => {
			bail!("Failed to create vulkan allocator: {:?}", e)
		}
	};
	info!("Vulkan allocator created");

	let pixel_format = egl.pixel_format();
	let _desired_fourcc: &[Fourcc] = if let 10 = pixel_format.color_bits {
		&[
			Fourcc::Abgr2101010,
			Fourcc::Argb2101010,
			Fourcc::Abgr8888,
			Fourcc::Argb8888,
		]
	} else {
		&[Fourcc::Abgr8888, Fourcc::Argb8888]
	};
	let _supported_formats = display.dmabuf_texture_formats(); // TODO: Ask stardust for supported formats
	let selected_format = Format {
		code: Fourcc::Abgr8888,
		modifier: smithay::backend::allocator::Modifier::Linear,
	};
	// let selected_format = desired_fourcc
	// 	.iter()
	// 	.find_map(|&f| supported_formats.iter().cloned().find(|&sf| sf.code == f))
	// 	.ok_or_else(|| eyre!("No supported dmabuf format found"))?;

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

	stardust_tx.blocking_send(WinitDisplayMessage::Render(
		buffers[buffer_to_render].clone(),
	))?;

	event_loop.run(move |event, _, control_flow| match event {
		Event::UserEvent(()) => {
			buffer_to_present.replace(buffer_to_render);
			buffer_to_render = (buffer_to_render + 1) % buffers.len();
			info!(
				"Rendering to {}",
				std::os::fd::AsRawFd::as_raw_fd(
					&buffers[buffer_to_render].handles().next().unwrap().clone()
				)
			);

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

			info!("Blitting {}", buffer_to_present);

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

fn init_egl(
	winit_window: Arc<Window>,
	attributes: GlAttributes,
) -> Result<(EGLDisplay, EGLContext, EGLSurface, bool)> {
	let display = EGLDisplay::new(winit_window.clone())?;

	let context =
		EGLContext::new_with_config(&display, attributes, PixelFormatRequirements::_10_bit())
			.or_else(|_| {
				EGLContext::new_with_config(&display, attributes, PixelFormatRequirements::_8_bit())
			})?;

	let (surface, is_x11) = if let Some(wl_surface) = winit_window.wayland_surface() {
		debug!("Winit backend: Wayland");
		let size = winit_window.inner_size();
		let surface = unsafe {
			wegl::WlEglSurface::new_from_raw(
				wl_surface as *mut _,
				size.width as i32,
				size.height as i32,
			)
		}
		.map_err(|e| eyre!(e.to_string()))?;
		(
			unsafe {
				EGLSurface::new(
					&display,
					context.pixel_format().unwrap(),
					context.config_id(),
					surface,
				)
				.map_err(egl::Error::CreationFailed)?
			},
			false,
		)
	} else if let Some(xlib_window) = winit_window.xlib_window().map(native::XlibWindow) {
		debug!("Winit backend: X11");
		(
			unsafe {
				EGLSurface::new(
					&display,
					context.pixel_format().unwrap(),
					context.config_id(),
					xlib_window,
				)
				.map_err(egl::Error::CreationFailed)?
			},
			true,
		)
	} else {
		unreachable!("No backends for winit other then Wayland and X11 are supported")
	};

	context.unbind()?;

	Ok((display, context, surface, is_x11))
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
