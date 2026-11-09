//! Bounded shm capture on the explicitly selected Wayland endpoint. No disk images.
use crate::{CapturedFrame, DesktopError, Result};
use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    time::{Duration, Instant},
};
use wayland_client::{
    protocol::{wl_buffer, wl_callback, wl_registry, wl_shm, wl_shm_pool},
    Connection, Dispatch, QueueHandle, WEnum,
};
mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::*;
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-toplevel-export-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocols/hyprland-toplevel-export-v1.xml");
}
use protocol::{
    hyprland_toplevel_export_frame_v1 as frame, hyprland_toplevel_export_manager_v1 as manager,
};
#[derive(Default)]
struct State {
    synchronized: bool,
    manager: Option<manager::HyprlandToplevelExportManagerV1>,
    shm: Option<wl_shm::WlShm>,
    format: Option<(wl_shm::Format, u32, u32, u32)>,
    buffer_done: bool,
    done: bool,
    failed: bool,
    inverted: bool,
}
impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        s: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        s.synchronized = true;
    }
}
impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        s: &mut Self,
        r: &wl_registry::WlRegistry,
        e: wl_registry::Event,
        _: &(),
        _: &Connection,
        q: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = e
        {
            match interface.as_str() {
                "wl_shm" => s.shm = Some(r.bind(name, 1, q, ())),
                "hyprland_toplevel_export_manager_v1" => {
                    s.manager = Some(r.bind(name, version.min(1), q, ()))
                }
                _ => (),
            }
        }
    }
}
impl Dispatch<frame::HyprlandToplevelExportFrameV1, ()> for State {
    fn event(
        s: &mut Self,
        _: &frame::HyprlandToplevelExportFrameV1,
        e: frame::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match e {
            frame::Event::Buffer {
                format: WEnum::Value(format),
                width,
                height,
                stride,
            } => s.format = Some((format, width, height, stride)),
            frame::Event::BufferDone => s.buffer_done = true,
            frame::Event::Ready { .. } => s.done = true,
            frame::Event::Failed => s.failed = true,
            frame::Event::Flags {
                flags: WEnum::Value(flags),
            } => s.inverted = flags.contains(frame::Flags::YInvert),
            _ => (),
        }
    }
}
wayland_client::delegate_noop!(State: ignore manager::HyprlandToplevelExportManagerV1);
wayland_client::delegate_noop!(State: ignore wl_shm::WlShm);
wayland_client::delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(State: ignore wl_buffer::WlBuffer);
fn unavailable(e: impl std::fmt::Display) -> DesktopError {
    DesktopError::Unavailable(e.to_string())
}
pub fn capture(display: &std::path::Path, handle: u32) -> Result<CapturedFrame> {
    use std::os::fd::AsRawFd;
    let conn = Connection::from_socket(UnixStream::connect(display).map_err(unavailable)?)
        .map_err(unavailable)?;
    let mut queue = conn.new_event_queue();
    let q = queue.handle();
    let _registry = conn.display().get_registry(&q, ());
    let _sync = conn.display().sync(&q, ());
    let mut state = State::default();
    let deadline = Instant::now() + Duration::from_millis(700);
    let pump = |queue: &mut wayland_client::EventQueue<State>, state: &mut State| -> Result<()> {
        queue.dispatch_pending(state).map_err(unavailable)?;
        conn.flush().map_err(unavailable)?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(DesktopError::Timeout)?;
        if let Some(read) = conn.prepare_read() {
            let mut fd = libc::pollfd {
                fd: conn.as_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let n = unsafe { libc::poll(&mut fd, 1, remaining.as_millis().max(1) as i32) };
            if n == 0 {
                return Err(DesktopError::Timeout);
            }
            if n < 0 {
                return Err(unavailable(std::io::Error::last_os_error()));
            }
            read.read().map_err(unavailable)?;
        }
        queue.dispatch_pending(state).map_err(unavailable)?;
        Ok(())
    };
    while !state.synchronized {
        pump(&mut queue, &mut state)?;
    }
    if state.manager.is_none() || state.shm.is_none() {
        return Err(DesktopError::Unsupported);
    }
    let frame = state
        .manager
        .as_ref()
        .unwrap()
        .capture_toplevel(0, handle, &q, ());
    while !state.buffer_done && !state.failed {
        pump(&mut queue, &mut state)?;
    }
    let (format, width, height, stride) = state.format.ok_or(DesktopError::Unsupported)?;
    if !matches!(format, wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888)
        || width == 0
        || height == 0
        || stride < width.saturating_mul(4)
    {
        return Err(DesktopError::Unsupported);
    }
    let size = (stride as usize)
        .checked_mul(height as usize)
        .filter(|n| *n <= 128 * 1024 * 1024)
        .ok_or(DesktopError::Unsupported)?;
    let file = tempfile::tempfile().map_err(unavailable)?;
    file.set_len(size as u64).map_err(unavailable)?;
    let map =
        unsafe { memmap2::MmapOptions::new().len(size).map_mut(&file) }.map_err(unavailable)?;
    let pool = state
        .shm
        .as_ref()
        .unwrap()
        .create_pool(file.as_fd(), size as i32, &q, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride as i32,
        format,
        &q,
        (),
    );
    frame.copy(&buffer, 1);
    while !state.done && !state.failed {
        pump(&mut queue, &mut state)?;
    }
    if state.failed {
        return Err(DesktopError::Unavailable("Window capture failed".into()));
    }
    let result = filtered_frame(&map, width, height, stride, state.inverted, 3840, 2160);
    frame.destroy();
    buffer.destroy();
    pool.destroy();
    state.manager.unwrap().destroy();
    Ok(result)
}

/// Retain supersampled detail for the preview. Average each source-pixel bucket
/// instead of dropping thin text strokes through nearest-neighbor decimation.
fn filtered_frame(
    source: &[u8],
    width: u32,
    height: u32,
    stride: u32,
    inverted: bool,
    max_width: u32,
    max_height: u32,
) -> CapturedFrame {
    let factor = (max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64)
        .min(1.);
    let w = (width as f64 * factor).max(1.) as u32;
    let h = (height as f64 * factor).max(1.) as u32;
    let mut rgba = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        let top = y as u64 * height as u64 / h as u64;
        let bottom = ((y + 1) as u64 * height as u64 / h as u64).max(top + 1);
        for x in 0..w {
            let left = x as u64 * width as u64 / w as u64;
            let right = ((x + 1) as u64 * width as u64 / w as u64).max(left + 1);
            let mut sums = [0u64; 3];
            for sy in top..bottom {
                let sy = if inverted { height as u64 - 1 - sy } else { sy };
                for sx in left..right {
                    let i = (sy * stride as u64 + sx * 4) as usize;
                    let pixel = u32::from_ne_bytes(source[i..i + 4].try_into().unwrap());
                    sums[0] += (pixel >> 16 & 255) as u64;
                    sums[1] += (pixel >> 8 & 255) as u64;
                    sums[2] += (pixel & 255) as u64;
                }
            }
            let count = (right - left) * (bottom - top);
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[
                (sums[0] / count) as u8,
                (sums[1] / count) as u8,
                (sums[2] / count) as u8,
                255,
            ]);
        }
    }
    CapturedFrame {
        width: w,
        height: h,
        stride: w as usize * 4,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filtering_preserves_thin_strokes_and_handles_stride_and_inversion() {
        let mut source = Vec::new();
        for pixel in [0xffffffffu32, 0xff000000, 0, 0xff000000, 0xffffffff, 0] {
            source.extend_from_slice(&pixel.to_ne_bytes());
        }
        let average = filtered_frame(&source, 2, 2, 12, false, 1, 1);
        assert_eq!(average.rgba, [127, 127, 127, 255]);
        let flipped = filtered_frame(&source, 2, 2, 12, true, 2, 2);
        assert_eq!(&flipped.rgba[..8], &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(flipped.stride, 8);
    }
    #[test]
    fn missing_selected_endpoint_never_falls_back_to_host() {
        let directory = tempfile::tempdir().unwrap();
        assert!(matches!(
            capture(&directory.path().join("private-display"), 1),
            Err(DesktopError::Unavailable(_))
        ));
    }
    #[test]
    fn silent_selected_compositor_has_a_bounded_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private-display");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let peer = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(1));
        });
        let start = Instant::now();
        assert!(matches!(capture(&path, 1), Err(DesktopError::Timeout)));
        assert!(start.elapsed() < Duration::from_secs(2));
        peer.join().unwrap();
    }
}
