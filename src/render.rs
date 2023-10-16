use std::os::fd::OwnedFd;

use smithay::backend::allocator::{
	dmabuf::{Dmabuf, DmabufFlags},
	Buffer,
};
use stardust_xr_fusion::{
	core::values::{BufferInfo, BufferPlaneInfo},
	items::camera::CameraItem,
};

pub async fn render(camera: &CameraItem, buffer: Dmabuf) {
	let modifier = buffer.format().modifier;
	let planes = buffer
		.strides()
		.zip(buffer.offsets())
		.enumerate()
		.map(|(idx, (stride, offset))| BufferPlaneInfo {
			idx: idx as u32,
			offset,
			stride,
			modifier: u64::from(modifier),
		})
		.collect();

	let mut flags = DmabufFlags::empty();
	if buffer.y_inverted() {
		flags.toggle(DmabufFlags::Y_INVERT);
	}

	let buffer_info = BufferInfo {
		height: buffer.height() as u32,
		width: buffer.width() as u32,
		fourcc: buffer.format().code as u32,
		flags: flags.bits(),
		planes: planes,
	};

	let fds: Vec<OwnedFd> = buffer
		.handles()
		.map(|fd| fd.try_clone_to_owned().unwrap())
		.collect();

	camera.render(buffer_info, fds).unwrap().await.unwrap();
}
