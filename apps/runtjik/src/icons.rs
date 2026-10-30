//! GTK theme lookup and GdkPixbuf rasterization use the same native libraries as the Python implementation.
use crate::util::*;
use anyhow::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    ptr,
};
#[link(name = "gtk-3")]
extern "C" {
    fn gtk_icon_theme_new() -> *mut libc::c_void;
    fn gtk_icon_theme_set_custom_theme(t: *mut libc::c_void, n: *const libc::c_char);
    fn gtk_icon_theme_lookup_icon(
        t: *mut libc::c_void,
        n: *const libc::c_char,
        size: libc::c_int,
        flags: libc::c_int,
    ) -> *mut libc::c_void;
    fn gtk_icon_info_get_filename(i: *mut libc::c_void) -> *const libc::c_char;
    fn gtk_icon_theme_get_search_path(
        t: *mut libc::c_void,
        path: *mut *mut *mut libc::c_char,
        n: *mut libc::c_int,
    );
}
#[link(name = "gobject-2.0")]
extern "C" {
    fn g_object_unref(p: *mut libc::c_void);
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_strfreev(p: *mut *mut libc::c_char);
}
fn stamp(p: &Path) -> Value {
    fs::metadata(p)
        .map(|m| {
            json!([
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
                m.len(),
                m.ino()
            ])
        })
        .unwrap_or(Value::Null)
}
pub struct Icons {
    theme: *mut libc::c_void,
    name: String,
    size: i32,
    cache: HashMap<String, String>,
    records: Value,
    manifest: PathBuf,
    dirty: bool,
}
impl Drop for Icons {
    fn drop(&mut self) {
        if !self.theme.is_null() {
            unsafe {
                g_object_unref(self.theme);
            }
        }
    }
}
impl Icons {
    pub fn new(settings: &Value) -> Self {
        let name = s(&settings["theme"]["icon_theme"]).to_owned();
        let size = settings["ui"]["icon_size"].as_i64().unwrap() as i32 * 2;
        let key = format!(
            "{:x}",
            Sha256::digest(format!(
                "{name}:{size}:{:?}:{:?}:{:?}",
                home(),
                std::env::var_os("XDG_DATA_HOME"),
                std::env::var_os("XDG_DATA_DIRS")
            ))
        );
        let manifest = xdg("cache").join("icons").join(format!("rust-{key}.json"));
        let mut records = json!({});
        if let Ok(v) = fs::read(&manifest) {
            if let Ok(v) = serde_json::from_slice::<Value>(&v) {
                if v["watched"].as_object().is_some_and(|w| {
                    !w.is_empty() && w.iter().all(|(p, v)| stamp(Path::new(p)) == *v)
                }) {
                    records = v["records"].clone();
                }
            }
        }
        Self {
            theme: ptr::null_mut(),
            name,
            size,
            cache: HashMap::new(),
            records,
            manifest,
            dirty: false,
        }
    }
    fn initialize(&mut self) -> Result<()> {
        if self.theme.is_null() {
            let name = CString::new(self.name.clone())?;
            unsafe {
                self.theme = gtk_icon_theme_new();
                gtk_icon_theme_set_custom_theme(self.theme, name.as_ptr());
            }
        }
        Ok(())
    }
    pub fn resolve(&mut self, name: &str) -> String {
        self.try_resolve(name).unwrap_or_else(|_| name.into())
    }
    fn try_resolve(&mut self, name: &str) -> Result<String> {
        if let Some(v) = self.cache.get(name) {
            return Ok(v.clone());
        }
        let r = &self.records[name];
        if let Some(source) = r["source"].as_str() {
            if stamp(Path::new(source)) == r["stamp"]
                && !r["stamp"].is_null()
                && Path::new(s(&r["resolved"])).is_file()
            {
                let v = s(&r["resolved"]).to_owned();
                self.cache.insert(name.into(), v.clone());
                return Ok(v);
            }
        }
        self.initialize()?;
        let mut path = if name.starts_with('/') {
            Some(PathBuf::from(name))
        } else {
            None
        };
        if path.is_none() {
            for candidate in name
                .split_whitespace()
                .chain(std::iter::once("application-x-executable"))
            {
                let n = CString::new(candidate)?;
                unsafe {
                    let info = gtk_icon_theme_lookup_icon(self.theme, n.as_ptr(), self.size, 16);
                    if !info.is_null() {
                        let p = gtk_icon_info_get_filename(info);
                        if !p.is_null() {
                            path =
                                Some(PathBuf::from(CStr::from_ptr(p).to_string_lossy().as_ref()));
                        }
                        g_object_unref(info);
                    }
                }
                if path.is_some() {
                    break;
                }
            }
        }
        let mut resolved = name.to_owned();
        if let Some(p) = path.filter(|p| p.is_file()) {
            resolved = p.to_string_lossy().to_string();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("svg")) {
                let key = format!(
                    "{:x}",
                    Sha256::digest(format!("{}:{}:{}", p.display(), stamp(&p), self.size))
                );
                let target = xdg("cache").join("icons").join(format!("rust-{key}.png"));
                if !target.is_file() {
                    private_dir(target.parent().unwrap())?;
                    let img =
                        gdk_pixbuf::Pixbuf::from_file_at_scale(&p, self.size, self.size, true)?;
                    let bytes = img.save_to_bufferv("png", &[])?;
                    atomic(&target, &bytes)?;
                }
                resolved = target.to_string_lossy().to_string();
            }
            self.records[name] = json!({"source":p,"stamp":stamp(&p),"resolved":resolved});
            self.dirty = true;
        }
        self.cache.insert(name.into(), resolved.clone());
        Ok(resolved)
    }
    pub fn flush(&mut self) -> Result<()> {
        if !self.dirty || self.theme.is_null() {
            return Ok(());
        }
        let mut roots = vec![];
        unsafe {
            let mut list = ptr::null_mut();
            let mut n = 0;
            gtk_icon_theme_get_search_path(self.theme, &mut list, &mut n);
            for i in 0..n {
                roots.push(PathBuf::from(
                    CStr::from_ptr(*list.add(i as usize))
                        .to_string_lossy()
                        .as_ref(),
                ));
            }
            g_strfreev(list);
        }
        let mut watched = json!({});
        for root in roots {
            watched[root.to_string_lossy().as_ref()] = stamp(&root);
            for node in walkdir::WalkDir::new(root)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let p = node.path();
                if node.file_type().is_dir() || node.file_name() == "index.theme" {
                    watched[p.to_string_lossy().as_ref()] = stamp(p);
                }
            }
        }
        atomic(
            &self.manifest,
            &serde_json::to_vec(&json!({"records":self.records,"watched":watched}))?,
        )?;
        self.dirty = false;
        Ok(())
    }
}
