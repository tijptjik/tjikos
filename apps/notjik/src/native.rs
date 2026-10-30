//! Small dynamically loaded C interfaces; no interpreter or Python extension ABI.
use anyhow::{bail, ensure, Result};
use std::{
    ffi::{CStr, CString},
    path::{Path, PathBuf},
};
pub struct Library(*mut libc::c_void);
impl Library {
    pub fn open(root: &Path, name: &str) -> Result<Self> {
        let env_name = format!("NOTJIK_{}_LIBRARY", name.to_uppercase());
        let paths = if let Some(p) = std::env::var_os(env_name) {
            vec![PathBuf::from(p)]
        } else {
            vec![
                root.join("lib").join(format!("lib{name}.so")),
                std::env::var_os("NOTJIK_NATIVE_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| crate::util::home().join(".local/share/notjik/native"))
                    .join(format!("lib{name}.so")),
                PathBuf::from(format!("lib{name}.so")),
            ]
        };
        for p in paths {
            let c = CString::new(p.as_os_str().as_encoded_bytes())?;
            let ptr = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if !ptr.is_null() {
                return Ok(Self(ptr));
            }
        }
        bail!("Native {name} library unavailable; run notjikctl telegram setup")
    }
    /// Caller must use the exact C ABI declared by the library for this symbol.
    pub unsafe fn symbol<T: Copy>(&self, name: &str) -> Result<T> {
        let c = CString::new(name)?;
        let p = libc::dlsym(self.0, c.as_ptr());
        ensure!(!p.is_null(), "Missing native symbol {name}");
        ensure!(
            std::mem::size_of::<T>() == std::mem::size_of_val(&p),
            "Invalid function pointer"
        );
        Ok(std::mem::transmute_copy(&p))
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}
pub struct Td {
    send: unsafe extern "C" fn(i32, *const libc::c_char),
    receive: unsafe extern "C" fn(f64) -> *const libc::c_char,
    execute: unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char,
    pub client: i32,
    _lib: Option<Library>,
}
impl Td {
    pub fn new(root: &Path) -> Result<Self> {
        let lib = Library::open(root, "tdjson")?;
        unsafe {
            let create: unsafe extern "C" fn() -> i32 = lib.symbol("td_create_client_id")?;
            let execute: unsafe extern "C" fn(*const libc::c_char) -> *const libc::c_char =
                lib.symbol("td_execute")?;
            let quiet =
                CString::new(r#"{"@type":"setLogVerbosityLevel","new_verbosity_level":0}"#)?;
            execute(quiet.as_ptr());
            let td = Self {
                send: lib.symbol("td_send")?,
                receive: lib.symbol("td_receive")?,
                execute,
                client: create(),
                _lib: Some(lib),
            };
            td.execute(
                &serde_json::json!({"@type":"setLogVerbosityLevel","new_verbosity_level":0}),
            )?;
            Ok(td)
        }
    }
    pub fn send(&self, v: &serde_json::Value) -> Result<()> {
        let c = CString::new(v.to_string())?;
        unsafe {
            (self.send)(self.client, c.as_ptr());
        }
        Ok(())
    }
    pub fn receive(&self, timeout: f64) -> Result<Option<serde_json::Value>> {
        let p = unsafe { (self.receive)(timeout) };
        if p.is_null() {
            Ok(None)
        } else {
            Ok(Some(serde_json::from_slice(
                unsafe { CStr::from_ptr(p) }.to_bytes(),
            )?))
        }
    }
    pub fn execute(&self, v: &serde_json::Value) -> Result<serde_json::Value> {
        let c = CString::new(v.to_string())?;
        let p = unsafe { (self.execute)(c.as_ptr()) };
        if p.is_null() {
            return Ok(serde_json::Value::Null);
        }
        Ok(serde_json::from_slice(
            unsafe { CStr::from_ptr(p) }.to_bytes(),
        )?)
    }
}

#[cfg(test)]
thread_local! {static CALLS:std::cell::RefCell<Vec<serde_json::Value>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
impl Td {
    pub fn mock() -> Self {
        unsafe extern "C" fn send(_: i32, p: *const libc::c_char) {
            let v = serde_json::from_slice(unsafe { CStr::from_ptr(p) }.to_bytes()).unwrap();
            CALLS.with(|q| q.borrow_mut().push(v));
        }
        unsafe extern "C" fn receive(_: f64) -> *const libc::c_char {
            std::ptr::null()
        }
        unsafe extern "C" fn execute(_: *const libc::c_char) -> *const libc::c_char {
            std::ptr::null()
        }
        CALLS.with(|q| q.borrow_mut().clear());
        Self {
            send,
            receive,
            execute,
            client: 1,
            _lib: None,
        }
    }
    pub fn calls() -> Vec<serde_json::Value> {
        CALLS.with(|q| q.borrow().clone())
    }
}
