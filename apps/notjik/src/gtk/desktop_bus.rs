//! Freedesktop Notifications adapter. Name acquisition never replaces an owner.
use crate::controller::Controller;
use anyhow::{ensure, Result};
use gtk::{gio, glib, prelude::*};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
const XML: &str = r#"<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u" direction="in"/></method>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
</interface></node>"#;
pub struct Bus {
    pub connection: gio::DBusConnection,
    registration: Option<gio::RegistrationId>,
    images: Rc<RefCell<std::collections::HashMap<u32, tempfile::NamedTempFile>>>,
}
impl Bus {
    pub fn acquire(controller: Rc<RefCell<Controller>>) -> Result<Self> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let result = connection.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
            Some(&(INTERFACE, 4u32).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
        )?;
        ensure!(
            result.get::<(u32,)>() == Some((1,)),
            "org.freedesktop.Notifications already has an owner"
        );
        let info = gio::DBusNodeInfo::for_xml(XML)?
            .lookup_interface(INTERFACE)
            .unwrap();
        let images = Rc::new(RefCell::new(std::collections::HashMap::new()));
        let retained = images.clone();
        let registration=connection.register_object(PATH,&info).method_call(move|_,_,_,_,method,params,invocation|{
 let result=(||->Result<glib::Variant>{match method {
 "GetCapabilities"=>Ok((vec!["actions","body","persistence","icon-static"],).to_variant()),
 "GetServerInformation"=>Ok(("Notjik","TjikOS",env!("CARGO_PKG_VERSION"),"1.2").to_variant()),
 "CloseNotification"=>{let (id,)=params.get::<(u32,)>().ok_or_else(||anyhow::anyhow!("Invalid id"))?;controller.borrow_mut().close(&format!("dbus-{id}"),3);Ok(().to_variant())},
 "Notify"=>{
 let app=params.child_get::<String>(0);let replace=params.child_get::<u32>(1);let icon=params.child_get::<String>(2);let title=params.child_get::<String>(3);let body=params.child_get::<String>(4);let actions=params.child_get::<Vec<String>>(5);let hints=params.child_get::<std::collections::HashMap<String,glib::Variant>>(6);let timeout=params.child_get::<i32>(7);
 ensure!(title.len()<=65536&&body.len()<=65536&&actions.len()<=32&&actions.len()%2==0,"Notification exceeds limits");
 let hint=|key:&str|hints.get(key).and_then(|v|v.get::<String>()).unwrap_or_default();
 let urgency=hints.get("urgency").and_then(|v|v.get::<u8>()).unwrap_or(1);let progress=hints.get("value").and_then(|v|v.get::<i32>());
 let mut image=hint("image-path");
 let cached = raw_image(&hints)?;
 if let Some(file) = &cached { image = file.path().to_string_lossy().into(); }
 let mut n=json!({"app":app,"desktopEntry":hint("desktop-entry"),"title":title,"body":body,"icon":icon,"image":image,"urgency":(["low","normal","critical"][urgency.min(2) as usize]),"type":if progress.is_some(){"progress"}else{"standard"},"data":progress.map(|v|json!({"value":v.clamp(0,100)})).unwrap_or(json!({})),"category":hint("category"),"requestedTimeout":timeout,"actions":actions.chunks_exact(2).map(|a|json!({"identifier":a[0],"label":if a[1].is_empty(){if a[0]=="default"{"Open"}else{&a[0]}}else{&a[1]}})).collect::<Vec<Value>>()});
 for key in ["resident","transient"]{n[key]=json!(hints.get(key).is_some_and(|v|v.get::<bool>().unwrap_or_else(||v.get::<i32>().unwrap_or(0)!=0)));}
 let id=controller.borrow_mut().notify(n,replace,super::host::now())?;
 if let Some(file) = cached { retained.borrow_mut().insert(id, file); }
 Ok((id,).to_variant())
 },_=>anyhow::bail!("Unknown method")}})();
 match result {Ok(v)=>invocation.return_value(Some(&v)),Err(e)=>invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs",&e.to_string())}
 }).build()?;
        Ok(Self {
            connection,
            registration: Some(registration),
            images,
        })
    }
    pub fn closed(&self, id: u32, reason: u32) {
        self.images.borrow_mut().remove(&id);
        let _ = self.connection.emit_signal(
            None,
            PATH,
            INTERFACE,
            "NotificationClosed",
            Some(&(id, reason).to_variant()),
        );
    }
    pub fn action(&self, id: u32, key: &str) {
        let _ = self.connection.emit_signal(
            None,
            PATH,
            INTERFACE,
            "ActionInvoked",
            Some(&(id, key).to_variant()),
        );
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        if let Some(id) = self.registration.take() {
            let _ = self.connection.unregister_object(id);
        }
        let _ = self.connection.call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ReleaseName",
            Some(&(INTERFACE,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        );
    }
}

fn raw_image(
    hints: &std::collections::HashMap<String, glib::Variant>,
) -> Result<Option<tempfile::NamedTempFile>> {
    let Some(v) = hints
        .get("image-data")
        .or_else(|| hints.get("image_data"))
        .or_else(|| hints.get("icon_data"))
    else {
        return Ok(None);
    };
    let (w, h, stride, alpha, bits, channels, data) = v
        .get::<(i32, i32, i32, bool, i32, i32, Vec<u8>)>()
        .ok_or_else(|| anyhow::anyhow!("Invalid image-data"))?;
    ensure!(
        w > 0
            && h > 0
            && w <= 4096
            && h <= 4096
            && bits == 8
            && channels == if alpha { 4 } else { 3 },
        "Unsupported image format"
    );
    ensure!(
        stride >= w * channels
            && stride <= 65536
            && data.len() <= 32 * 1024 * 1024
            && data.len() >= ((h - 1) as usize * stride as usize + w as usize * channels as usize),
        "Invalid image stride or length"
    );
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(data),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        alpha,
        bits,
        w,
        h,
        stride,
    );
    let root = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?).join("notjik-images");
    crate::util::private_dir(&root)?;
    let file = tempfile::Builder::new().suffix(".png").tempfile_in(root)?;
    pixbuf.savev(file.path(), "png", &[])?;
    Ok(Some(file))
}
