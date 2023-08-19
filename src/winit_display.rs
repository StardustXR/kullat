use color_eyre::eyre::{bail, eyre, Result};
use smithay::{
	backend::{
		allocator::{
			dmabuf::{Dmabuf, DmabufAllocator},
			vulkan::{self, ImageUsageFlags, VulkanAllocator},
			Fourcc,
		},
		egl::{
			context::{GlAttributes, PixelFormatRequirements},
			display::EGLDisplay,
			native, EGLContext, EGLSurface, Error as EGLError,
		},
		renderer::{gles::GlesRenderer, Bind, Blit, TextureFilter},
		vulkan::{version::Version, Instance, PhysicalDevice},
		winit::WindowSize,
	},
	reexports::{
		ash::vk::ExtPhysicalDeviceDrmFn,
		winit::{
			dpi::LogicalSize,
			event::{Event, WindowEvent},
			event_loop::EventLoopBuilder,
			platform::{
				wayland::{EventLoopBuilderExtWayland, WindowExtWayland},
				x11::WindowExtX11,
			},
			window::{Window, WindowBuilder},
		},
	},
	utils::{Physical, Rectangle, Size},
};
use std::{
	cell::{Cell, RefCell},
	rc::Rc,
	sync::Arc,
};
use tokio::sync::mpsc::{Receiver, Sender};
use wayland_egl as wegl;

use tracing::{debug, error, info, info_span, trace};

pub fn start(display_rx: Receiver<()>, stardust_tx: Sender<()>) -> Result<()> {
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

	let event_loop = EventLoopBuilder::new().with_any_thread(true).build();
	let winit_window = Arc::new(
		window_builder
			.build(&event_loop)
			.map_err(|e| eyre!(e.to_string()))?,
	);

	span.record("window", Into::<u64>::into(winit_window.id()));
	debug!("Window created");

	let (w, h): (u32, u32) = winit_window.inner_size().into();
	let size = Rc::new(RefCell::new(WindowSize {
		physical_size: (w as i32, h as i32).into(),
		scale_factor: winit_window.scale_factor(),
	}));

	let (display, context, surface, _is_x11) = init_egl(winit_window, attributes)?;
	let pixel_format = surface.pixel_format();
	let desired_fourcc: &[Fourcc] = if let 10 = pixel_format.color_bits {
		&[
			Fourcc::Abgr2101010,
			Fourcc::Argb2101010,
			Fourcc::Abgr8888,
			Fourcc::Argb8888,
		]
	} else {
		&[Fourcc::Abgr8888, Fourcc::Argb8888]
	};
	let supported_formats = display.dmabuf_texture_formats(); // TODO: Ask stardust for supported formats
	let selected_format = desired_fourcc
		.iter()
		.find_map(|&f| supported_formats.iter().find(|&sf| sf.code == f))
		.ok_or_else(|| eyre!("No supported dmabuf format found"))?;

	let egl = Rc::new(surface);
	let mut renderer: GlesRenderer = unsafe { GlesRenderer::new(context)?.into() };

	let mut allocator = match vulkan_allocator() {
		Ok(allocator) => allocator,
		Err(e) => {
			bail!("Failed to create vulkan allocator: {:?}", e)
		}
	};
	error!("Vulkan allocator created");

	let vk_image = allocator.0.create_buffer_with_usage(
		size.borrow().physical_size.w.try_into()?,
		size.borrow().physical_size.h.try_into()?,
		selected_format.code,
		&[selected_format.modifier],
		ImageUsageFlags::SAMPLED,
	);

	// State for the event loop
	let mut buffer_to_present: Option<Dmabuf> = None;
	let mut resized = false;

	event_loop.run(move |event, _, control_flow| match event {
		Event::RedrawRequested(_id) => {
			if buffer_to_present.is_none() {
				return;
			};

			renderer.bind(egl.clone()).unwrap();

			if resized {
				let size = size.borrow().physical_size;
				egl.resize(size.w, size.h, 0, 0);
			}

			renderer
				.blit_from(
					buffer_to_present.clone().unwrap(),
					Rectangle::from_loc_and_size((0, 0), (1, 1)),
					Rectangle::from_loc_and_size((0, 0), (1, 1)),
					TextureFilter::Linear,
				)
				.unwrap();
			egl.swap_buffers(None).unwrap();
		}
		Event::WindowEvent { event, .. } => match event {
			WindowEvent::Resized(psize) => {
				trace!("Resizing window to {:?}", psize);

				buffer_to_present = None;

				let mut size = size.borrow_mut();
				let (pw, ph): (u32, u32) = psize.into();
				size.physical_size = (pw as i32, ph as i32).into();

				resized = true;
			}

			WindowEvent::ScaleFactorChanged {
				scale_factor,
				new_inner_size: new_psize,
			} => {
				let mut size = size.borrow_mut();
				let (pw, ph): (u32, u32) = (*new_psize).into();
				size.physical_size = (pw as i32, ph as i32).into();
				size.scale_factor = scale_factor;

				resized = true;
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
				.map_err(EGLError::CreationFailed)?
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
				.map_err(EGLError::CreationFailed)?
			},
			true,
		)
	} else {
		unreachable!("No backends for winit other then Wayland and X11 are supported")
	};

	let _ = context.unbind();

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
