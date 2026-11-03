use crate::{native::Library, util::run};
use anyhow::{bail, ensure, Result};
use serde_json::Value;
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn converter_command() -> Command {
    // The bridge can outlive an atomic app update. current_exe() then resolves
    // to a deleted pathname; procfs still opens the running executable's inode.
    // Keep the converter on the bridge's version until the bridge restarts.
    let mut command = Command::new("/proc/self/exe");
    command.arg("media-render");
    command
}

pub fn render(root: &Path, source: &Path, output: &Path, format: &str) -> Result<()> {
    for (resource, limit) in [
        (libc::RLIMIT_CPU, 20),
        (libc::RLIMIT_FSIZE, 20 * 1024 * 1024),
        (libc::RLIMIT_AS, 1536 * 1024 * 1024),
    ] {
        let r = libc::rlimit {
            rlim_cur: limit,
            rlim_max: limit,
        };
        ensure!(
            unsafe { libc::setrlimit(resource, &r) } == 0,
            "Cannot constrain media renderer"
        );
    }
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-y",
        "-threads",
        "1",
        "-filter_complex_threads",
        "1",
    ]);
    let palette =
        "split[a][b];[a]palettegen=reserve_transparent=1[p];[b][p]paletteuse=alpha_threshold=128";
    match format {
        "webp" => {
            run(
                cmd.arg("-i")
                    .arg(source)
                    .args([
                        "-frames:v",
                        "1",
                        "-vf",
                        "scale=512:512:force_original_aspect_ratio=decrease",
                        "-f",
                        "image2",
                        "-update",
                        "1",
                        "-c:v",
                        "png",
                    ])
                    .arg(output),
                Duration::from_secs(15),
            )?;
        }
        "webm" => {
            run(
                cmd.args(["-c:v", "libvpx-vp9", "-i"])
                    .arg(source)
                    .args([
                        "-t",
                        "10",
                        "-an",
                        "-filter_complex",
                        &format!(
                            "fps=30,scale=256:256:force_original_aspect_ratio=decrease,{palette}"
                        ),
                        "-loop",
                        "0",
                        "-f",
                        "gif",
                    ])
                    .arg(output),
                Duration::from_secs(25),
            )?;
        }
        "tgs" => {
            let mut raw = Vec::new();
            flate2::read::GzDecoder::new(File::open(source)?)
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut raw)?;
            ensure!(raw.len() <= 4 * 1024 * 1024, "Sticker data too large");
            let data: Value = serde_json::from_slice(&raw)?;
            ensure!(
                !data["assets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|a| crate::util::truthy(&a["p"]) || crate::util::truthy(&a["u"])),
                "External sticker assets are unsupported"
            );
            let lib = Library::open(root, "rlottie")?;
            type Ptr = *mut libc::c_void;
            unsafe {
                let create: unsafe extern "C" fn(
                    *const libc::c_char,
                    *const libc::c_char,
                    *const libc::c_char,
                ) -> Ptr = lib.symbol("lottie_animation_from_data")?;
                let destroy: unsafe extern "C" fn(Ptr) = lib.symbol("lottie_animation_destroy")?;
                let rate: unsafe extern "C" fn(Ptr) -> f64 =
                    lib.symbol("lottie_animation_get_framerate")?;
                let total: unsafe extern "C" fn(Ptr) -> usize =
                    lib.symbol("lottie_animation_get_totalframe")?;
                let size: unsafe extern "C" fn(Ptr, *mut usize, *mut usize) =
                    lib.symbol("lottie_animation_get_size")?;
                let render: unsafe extern "C" fn(Ptr, usize, *mut u32, usize, usize, usize) =
                    lib.symbol("lottie_animation_render")?;
                let json = CString::new(data.to_string())?;
                let empty = CString::new("")?;
                let ptr = create(json.as_ptr(), empty.as_ptr(), empty.as_ptr());
                ensure!(!ptr.is_null(), "Invalid sticker animation");
                struct Animation(Ptr, unsafe extern "C" fn(Ptr));
                impl Drop for Animation {
                    fn drop(&mut self) {
                        unsafe { (self.1)(self.0) }
                    }
                }
                let _animation = Animation(ptr, destroy);
                let rate = rate(ptr);
                let total = total(ptr);
                ensure!(
                    rate.is_finite() && rate > 0. && total > 0 && total as f64 / rate <= 10.,
                    "Invalid sticker duration"
                );
                let (mut w, mut h) = (0, 0);
                size(ptr, &mut w, &mut h);
                ensure!(
                    w > 0 && h > 0 && w.max(h) <= 4096,
                    "Invalid sticker dimensions"
                );
                let scale = 1f64.min(256. / w.max(h) as f64);
                w = (w as f64 * scale).round().max(1.) as usize;
                h = (h as f64 * scale).round().max(1.) as usize;
                let fps = rate.min(30.);
                let frames = (total as f64 / rate * fps).ceil().max(1.) as usize;
                cmd.args([
                    "-f",
                    "rawvideo",
                    "-pixel_format",
                    "bgra",
                    "-video_size",
                    &format!("{w}x{h}"),
                    "-framerate",
                    &fps.to_string(),
                    "-i",
                    "pipe:0",
                    "-filter_complex",
                    palette,
                    "-loop",
                    "0",
                    "-f",
                    "gif",
                ])
                .arg(output);
                let mut child = cmd
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                let result = (|| -> Result<()> {
                    let mut input = child.stdin.take().unwrap();
                    let mut pixels = vec![0u32; w * h];
                    for frame in 0..frames {
                        render(
                            ptr,
                            ((frame as f64 * rate / fps) as usize).min(total - 1),
                            pixels.as_mut_ptr(),
                            w,
                            h,
                            w * 4,
                        );
                        input.write_all(std::slice::from_raw_parts(
                            pixels.as_ptr() as *const u8,
                            pixels.len() * 4,
                        ))?;
                    }
                    drop(input);
                    let start = Instant::now();
                    loop {
                        if let Some(status) = child.try_wait()? {
                            ensure!(status.success(), "Sticker conversion failed");
                            break;
                        }
                        ensure!(
                            start.elapsed() < Duration::from_secs(15),
                            "Sticker conversion timed out"
                        );
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Ok(())
                })();
                if result.is_err() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                result?;
            }
        }
        _ => bail!("Unsupported conversion format"),
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converter_survives_executable_replacement() {
        const CHILD: &str = "NOTJIK_TEST_REPLACED_CONVERTER";
        const TEST: &str = "media::tests::converter_survives_executable_replacement";
        if std::env::var_os(CHILD).is_some() {
            let executable = std::env::current_exe().unwrap();
            // Reproduce the installer's atomic replacement while this copy runs.
            let replacement = executable.with_extension("pending");
            std::fs::copy("/usr/bin/false", &replacement).unwrap();
            std::fs::rename(replacement, &executable).unwrap();
            assert!(!std::env::current_exe().unwrap().exists());
            let command = converter_command();
            // This is a Rust test executable, so inspect its test list instead
            // of passing the production CLI's media-render argument.
            let result = Command::new(command.get_program())
                .arg("--list")
                .output()
                .unwrap();
            assert!(result.status.success());
            assert!(String::from_utf8_lossy(&result.stdout).contains(TEST));
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("notjik-test");
        std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        let result = Command::new(executable)
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
