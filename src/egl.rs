use color_eyre::eyre::{eyre, Result};
use smithay::backend::egl::{
	self,
	context::{GlAttributes, PixelFormatRequirements},
	display::{EGLDisplay, EGLDisplayHandle},
	ffi,
	native::{EGLNativeSurface, XlibWindow},
	wrap_egl_call_ptr, EGLContext, EGLError, EGLSurface,
};
use std::{
	os::raw::{c_int, c_ulong, c_void},
	sync::Arc,
};
use wayland_egl::WlEglSurface;
use winit::{
	platform::{wayland::WindowExtWayland, x11::WindowExtX11},
	window::Window,
};

use tracing::debug;

pub fn init_egl(
	winit_window: Arc<Window>,
	attributes: GlAttributes,
) -> Result<(EGLDisplay, EGLContext, EGLSurface, bool)> {
	let display = EGLDisplay::new(winit_window.clone())?;

	let context =
		EGLContext::new_with_config(&display, attributes, PixelFormatRequirements::_8_bit())?;

	let (surface, is_x11) = if let Some(wl_surface) = winit_window.wayland_surface() {
		debug!("Winit backend: Wayland");
		let size = winit_window.inner_size();
		let surface: WlEglSurfaceSRGB = unsafe {
			WlEglSurface::new_from_raw(wl_surface as *mut _, size.width as i32, size.height as i32)
		}
		.map_err(|e| eyre!(e.to_string()))?
		.into();
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
	} else if let Some(xlib_window) = winit_window.xlib_window().map(XlibWindowSRGB::from) {
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

static WINIT_SURFACE_ATTRIBUTES_SRGB: [c_int; 5] = [
	ffi::egl::GL_COLORSPACE as c_int,
	ffi::egl::GL_COLORSPACE_SRGB as c_int,
	ffi::egl::RENDER_BUFFER as c_int,
	ffi::egl::BACK_BUFFER as c_int,
	ffi::egl::NONE as c_int,
];

#[derive(Debug)]
pub struct XlibWindowSRGB(pub XlibWindow);

impl From<u64> for XlibWindowSRGB {
	fn from(window: c_ulong) -> Self {
		XlibWindowSRGB(XlibWindow(window))
	}
}

impl From<XlibWindow> for XlibWindowSRGB {
	fn from(window: XlibWindow) -> Self {
		XlibWindowSRGB(window)
	}
}

impl Into<XlibWindow> for XlibWindowSRGB {
	fn into(self) -> XlibWindow {
		self.0
	}
}

unsafe impl EGLNativeSurface for XlibWindowSRGB {
	unsafe fn create(
		&self,
		display: &Arc<EGLDisplayHandle>,
		config_id: ffi::egl::types::EGLConfig,
	) -> Result<*const c_void, EGLError> {
		wrap_egl_call_ptr(|| unsafe {
			let mut id = self.0 .0;
			ffi::egl::CreatePlatformWindowSurfaceEXT(
				display.handle,
				config_id,
				(&mut id) as *mut c_ulong as *mut _,
				WINIT_SURFACE_ATTRIBUTES_SRGB.as_ptr(),
			)
		})
	}

	fn identifier(&self) -> Option<String> {
		Some("Winit/X11 sRGB".into())
	}
}

#[derive(Debug)]
pub struct WlEglSurfaceSRGB(pub WlEglSurface);

impl From<WlEglSurface> for WlEglSurfaceSRGB {
	fn from(surface: WlEglSurface) -> Self {
		WlEglSurfaceSRGB(surface)
	}
}

impl Into<WlEglSurface> for WlEglSurfaceSRGB {
	fn into(self) -> WlEglSurface {
		self.0
	}
}

unsafe impl EGLNativeSurface for WlEglSurfaceSRGB {
	unsafe fn create(
		&self,
		display: &Arc<EGLDisplayHandle>,
		config_id: ffi::egl::types::EGLConfig,
	) -> Result<*const c_void, EGLError> {
		wrap_egl_call_ptr(|| unsafe {
			ffi::egl::CreatePlatformWindowSurfaceEXT(
				display.handle,
				config_id,
				self.0.ptr() as *mut _,
				WINIT_SURFACE_ATTRIBUTES_SRGB.as_ptr(),
			)
		})
	}

	fn resize(&self, width: i32, height: i32, dx: i32, dy: i32) -> bool {
		WlEglSurface::resize(&self.0, width, height, dx, dy);
		true
	}

	fn identifier(&self) -> Option<String> {
		Some("Winit/Wayland sRGB".into())
	}
}
