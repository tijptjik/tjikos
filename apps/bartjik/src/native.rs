//! Toolkit-independent MPRIS/SNI state and actions. One session connection per core.
use crate::util::{arr, item, s, st};
use anyhow::{ensure, Context, Result};
use gio::glib::{self, variant::ToVariant};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
const DBUS: &str = "org.freedesktop.DBus";
const PROPS: &str = "org.freedesktop.DBus.Properties";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const SNI: &str = "org.kde.StatusNotifierItem";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const MPATH: &str = "/org/mpris/MediaPlayer2";
fn call(
    c: &gio::DBusConnection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    args: &glib::Variant,
) -> Result<glib::Variant> {
    Ok(c.call_sync(
        Some(dest),
        path,
        iface,
        method,
        Some(args),
        None,
        gio::DBusCallFlags::NONE,
        1200,
        gio::Cancellable::NONE,
    )?)
}
pub fn value(v: &glib::Variant) -> Value {
    let ty = v.type_().as_str();
    if ty == "v" {
        return v
            .as_variant()
            .map(|inner| value(&inner))
            .unwrap_or(Value::Null);
    }
    match ty {
        "s" | "o" | "g" => json!(v.str().unwrap_or("")),
        "b" => json!(v.get::<bool>()),
        "y" => json!(v.get::<u8>()),
        "i" => json!(v.get::<i32>()),
        "u" => json!(v.get::<u32>()),
        "x" => json!(v.get::<i64>()),
        "t" => json!(v.get::<u64>()),
        "n" => json!(v.get::<i16>()),
        "q" => json!(v.get::<u16>()),
        "d" => json!(v.get::<f64>()),
        _ if ty.starts_with("a{") => {
            let mut result = serde_json::Map::new();
            for i in 0..v.n_children() {
                let pair = v.child_value(i);
                result.insert(
                    pair.child_value(0).str().unwrap_or("").into(),
                    value(&pair.child_value(1)),
                );
            }
            Value::Object(result)
        }
        _ if v.is_container() => Value::Array(
            (0..v.n_children())
                .map(|i| value(&v.child_value(i)))
                .collect(),
        ),
        _ => Value::Null,
    }
}
fn all(c: &gio::DBusConnection, dest: &str, path: &str, iface: &str) -> Result<Value> {
    Ok(value(
        &call(c, dest, path, PROPS, "GetAll", &(iface,).to_variant())?.child_value(0),
    ))
}
fn owner(c: &gio::DBusConnection, name: &str) -> Result<String> {
    call(
        c,
        DBUS,
        "/org/freedesktop/DBus",
        DBUS,
        "GetNameOwner",
        &(name,).to_variant(),
    )?
    .child_value(0)
    .str()
    .map(str::to_owned)
    .context("Service unavailable")
}
fn names(c: &gio::DBusConnection) -> Result<Vec<String>> {
    call(
        c,
        DBUS,
        "/org/freedesktop/DBus",
        DBUS,
        "ListNames",
        &().to_variant(),
    )?
    .child_value(0)
    .get()
    .context("Invalid bus names")
}
fn endpoint(name: &str) -> (String, String) {
    name.split_once('/')
        .map(|(s, p)| (s.into(), format!("/{p}")))
        .unwrap_or((name.into(), "/StatusNotifierItem".into()))
}
struct Bus {
    connection: gio::DBusConnection,
    registered: RefCell<Vec<String>>,
    observed_owners: RefCell<Vec<String>>,
    host_registered: RefCell<String>,
    dirty: Cell<bool>,
    send: Box<dyn Fn(Value)>,
}
impl Bus {
    fn schedule(self: &Rc<Self>) {
        if self.dirty.replace(true) {
            return;
        }
        let bus = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(25), move || {
            bus.dirty.set(false);
            if let Err(error) = bus.refresh() {
                (bus.send)(json!({"players":[], "tray":[], "error":error.to_string()}));
            }
        });
    }
    fn refresh(&self) -> Result<()> {
        let _profile = crate::profile::span("native.refresh");
        ensure!(!self.connection.is_closed(), "Session bus disconnected");
        let c = &self.connection;
        let mut players = vec![];
        for name in names(c)?
            .into_iter()
            .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
            .take(32)
        {
            if let (Ok(owner), Ok(props), Ok(root)) = (
                owner(c, &name),
                all(c, &name, MPATH, PLAYER),
                all(c, &name, MPATH, "org.mpris.MediaPlayer2"),
            ) {
                players.push(json!({"service":name,"owner":owner,"props":props,"root":root}));
            }
        }
        let watcher_owner = owner(c, WATCHER).unwrap_or_default();
        let own = c.unique_name().map(|n| n.to_string()).unwrap_or_default();
        let roster = if watcher_owner == own {
            self.registered.borrow().clone()
        } else if watcher_owner.is_empty() {
            vec![]
        } else {
            all(c, WATCHER, "/StatusNotifierWatcher", WATCHER)
                .ok()
                .map(|v| {
                    arr(&v["RegisteredStatusNotifierItems"])
                        .iter()
                        .map(st)
                        .collect()
                })
                .unwrap_or_default()
        };
        if !watcher_owner.is_empty()
            && watcher_owner != own
            && *self.host_registered.borrow() != watcher_owner
        {
            call(
                c,
                WATCHER,
                "/StatusNotifierWatcher",
                WATCHER,
                "RegisterStatusNotifierHost",
                &(own.as_str(),).to_variant(),
            )?;
            *self.host_registered.borrow_mut() = watcher_owner.clone();
        }
        let mut tray = vec![];
        for name in roster.into_iter().take(64) {
            let (service, path) = endpoint(&name);
            if let (Ok(owner), Ok(mut props)) = (owner(c, &service), all(c, &service, &path, SNI)) {
                // Bound wire payloads and retain one useful SNI pixel buffer per icon role.
                for key in ["IconPixmap", "AttentionIconPixmap", "OverlayIconPixmap"] {
                    props[key] = best_pixmap(&props[key]);
                }
                tray.push(json!({"id":format!("{owner}{path}"),"service":service,"owner":owner,"path":path,"props":props}));
            }
        }
        *self.observed_owners.borrow_mut() = players
            .iter()
            .chain(tray.iter())
            .filter_map(|v| v["owner"].as_str().map(str::to_owned))
            .collect();
        (self.send)(
            json!({"players":players,"tray":tray,"watcher":if watcher_owner==own{"owned"}else{"external"},"error":""}),
        );
        Ok(())
    }
}
fn best_pixmap(v: &Value) -> Value {
    arr(v)
        .iter()
        .filter(|p| {
            let w = p[0].as_u64().unwrap_or(0);
            let h = p[1].as_u64().unwrap_or(0);
            w > 0 && h > 0 && w <= 256 && h <= 256 && arr(&p[2]).len() as u64 == w * h * 4
        })
        .min_by_key(|p| (p[0].as_i64().unwrap_or(0) - 32).abs())
        .cloned()
        .unwrap_or(Value::Null)
}
/// A read-only host uses an existing watcher; our queued name takes over only if absent.
#[allow(deprecated)]
pub fn watch(send: impl Fn(Value) + Send + Sync + 'static) {
    let send = std::sync::Arc::new(send);
    std::thread::spawn(move || loop {
        let context = glib::MainContext::default();
        let result=context.with_thread_default(||->Result<()>{
 let c=gio::bus_get_sync(gio::BusType::Session,gio::Cancellable::NONE)?;
 let bus=Rc::new(Bus{connection:c.clone(),registered:RefCell::new(vec![]),observed_owners:RefCell::new(vec![]),host_registered:RefCell::new(String::new()),dirty:Cell::new(false),send:{let send=send.clone();Box::new(move|value|send(value))}});
 // The send closure cannot borrow across callbacks, so this scope owns the loop.
 let xml=r#"<node><interface name='org.kde.StatusNotifierWatcher'><method name='RegisterStatusNotifierItem'><arg type='s' direction='in'/></method><method name='RegisterStatusNotifierHost'><arg type='s' direction='in'/></method><property name='RegisteredStatusNotifierItems' type='as' access='read'/><property name='IsStatusNotifierHostRegistered' type='b' access='read'/><property name='ProtocolVersion' type='i' access='read'/><signal name='StatusNotifierItemRegistered'><arg type='s'/></signal><signal name='StatusNotifierItemUnregistered'><arg type='s'/></signal><signal name='StatusNotifierHostRegistered'/></interface></node>"#;
 let info=gio::DBusNodeInfo::for_xml(xml)?;let iface=info.lookup_interface(WATCHER).unwrap();let b=bus.clone();let prop=bus.clone();
 let registration=c.register_object("/StatusNotifierWatcher",&iface).method_call(move|connection,sender,_,_,method,args,reply|{
  let name=args.child_value(0).str().unwrap_or("").to_owned();
  if method=="RegisterStatusNotifierItem" {
   let key=if name.starts_with('/'){format!("{}{name}",sender.unwrap_or(""))}else{format!("{name}/StatusNotifierItem")};
   if !b.registered.borrow().contains(&key){b.registered.borrow_mut().push(key.clone());let _=connection.emit_signal(None,"/StatusNotifierWatcher",WATCHER,"StatusNotifierItemRegistered",Some(&(key,).to_variant()));}
  } else if method=="RegisterStatusNotifierHost" {let _=connection.emit_signal(None,"/StatusNotifierWatcher",WATCHER,"StatusNotifierHostRegistered",Some(&().to_variant()));}
  b.schedule();reply.return_value(Some(&().to_variant()));
 }).property(move|_,_,_,_,property|match property{"RegisteredStatusNotifierItems"=>prop.registered.borrow().clone().to_variant(),"IsStatusNotifierHostRegistered"=>true.to_variant(),_=>0i32.to_variant()}).build()?;
 let b=bus.clone();let lost=bus.clone();let ownership=gio::bus_own_name_on_connection(&c,WATCHER,gio::BusNameOwnerFlags::NONE,move|_,_|b.schedule(),move|_,_|{lost.host_registered.borrow_mut().clear();lost.schedule();});
 let subscriptions: Vec<_> = [PROPS, SNI, WATCHER, DBUS, "com.canonical.dbusmenu"].into_iter().map(|interface| {
 let b=bus.clone();c.signal_subscribe(None,Some(interface),if interface==DBUS {Some("NameOwnerChanged")} else {None},None,None,gio::DBusSignalFlags::NONE,move|connection,sender,path,interface,member,args|{
  if interface=="com.canonical.dbusmenu" {(b.send)(json!({"kind":"menuChanged","owner":sender,"path":path}));}
  // Unrelated PropertiesChanged and transient helper bus connections must not
  // trigger full media/tray rereads (including expensive pixmap decoding).
  if interface==WATCHER || ([PROPS,SNI].contains(&interface)
      && b.observed_owners.borrow().iter().any(|v| v==sender)) {b.schedule();}
  if interface==DBUS && member=="NameOwnerChanged" {
   let (name,old,_new)=args.get::<(String,String,String)>().unwrap_or_default();
   if name==WATCHER || name.starts_with("org.mpris.MediaPlayer2.")
       || b.observed_owners.borrow().contains(&old)
       || b.registered.borrow().iter().any(|v| endpoint(v).0==name) {b.schedule();}
  }
  if interface==DBUS && member=="NameOwnerChanged"{let (name,_old,new)=args.get::<(String,String,String)>().unwrap_or_default();if new.is_empty(){let removed:Vec<_>=b.registered.borrow().iter().filter(|v|endpoint(v).0==name).cloned().collect();b.registered.borrow_mut().retain(|v|endpoint(v).0!=name);for name in removed{let _=connection.emit_signal(None,"/StatusNotifierWatcher",WATCHER,"StatusNotifierItemUnregistered",Some(&(name,).to_variant()));}}}
 })}).collect();
 let loop_ = glib::MainLoop::new(Some(&context), false);
 let done = loop_.clone();
 c.connect_closed(move |_, _, _| done.quit());
 bus.schedule();
 loop_.run();
 for subscription in subscriptions { c.signal_unsubscribe(subscription); }
 gio::bus_unown_name(ownership);
 let _ = c.unregister_object(registration);
 Ok(())
 });
        match result {
            Ok(Ok(())) => (),
            Ok(Err(e)) => send(json!({"players":[],"tray":[],"error":e.to_string()})),
            Err(e) => send(json!({"players":[],"tray":[],"error":e.to_string()})),
        };
        std::thread::sleep(Duration::from_secs(1));
    });
}
fn matches(pattern: &str, names: &[&str]) -> bool {
    let p = pattern.to_lowercase();
    names.iter().any(|n| {
        let n = n.to_lowercase();
        p.strip_suffix('*')
            .map(|p| n.starts_with(p))
            .unwrap_or(n == p)
    })
}
pub fn items(module: &Value, native: &Value) -> Vec<Value> {
    if module["provider"] == "icontray" {
        return arr(&native["tray"]).iter().filter_map(|t|{let p=&t["props"];let names=[s(&p["Id"]),s(&p["Title"]),s(&p["ToolTip"][2])];let excluded=module["exclude"].as_array().cloned().unwrap_or(vec![json!("Input Method")]);if excluded.iter().any(|v|matches(s(v),&names)){return None;}
 let urgent=crate::tray::unread(s(&p["Id"]),&{let mut props=p.clone();props["IconPixmap"]=json!([p["IconPixmap"]]);props});
 let mut row=item(s(&t["id"]),None,Some(s(&p["IconName"])),"theme",json!({"urgent":urgent}),&format!("{}\n{}",s(&p["ToolTip"][2]),s(&p["ToolTip"][3])).trim(),json!({"leftClick":"tray.activate","rightClick":"tray.menu","middleClick":"tray.secondary","scrollUp":"tray.up","scrollDown":"tray.down"}));
 let override_=module["iconOverrides"].as_object().and_then(|o|o.iter().find(|(key,_)|matches(key,&names))).map(|(_,v)|s(v).to_owned());
 if let Some(path)=override_ {row["icon"]["kind"]=json!("image");row["icon"]["value"]=json!(path.strip_prefix("builtin:").map(|n|crate::util::root().join(format!("assets/icons/tray/{n}.svg")).to_string_lossy().into_owned()).unwrap_or(path));}
 else{let attention=urgent && (!s(&p["AttentionIconName"]).is_empty()||!p["AttentionIconPixmap"].is_null());let pix=if attention{&p["AttentionIconPixmap"]}else{&p["IconPixmap"]};let name=if attention{s(&p["AttentionIconName"])}else{s(&p["IconName"])};row["icon"]["value"]=json!(name);if name.starts_with('/') {row["icon"]["kind"]=json!("image");}else if !pix.is_null(){row["icon"]["kind"]=json!("pixmap");row["icon"]["pixmap"]=pix.clone();}row["icon"]["overlay"]=p["OverlayIconPixmap"].clone();row["icon"]["overlayName"]=p["OverlayIconName"].clone();}
 row["native"]=t.clone();Some(row)
 }).collect();
    }
    if module["provider"] != "playerctl" {
        return vec![];
    }
    let mut players: Vec<_> = arr(&native["players"])
        .iter()
        .filter(|p| {
            arr(&module["players"]).is_empty()
                || arr(&module["players"]).iter().any(|name| {
                    s(&p["root"]["Identity"]).eq_ignore_ascii_case(s(name))
                        || s(&p["root"]["DesktopEntry"]).eq_ignore_ascii_case(s(name))
                        || s(&p["service"])
                            .to_lowercase()
                            .contains(&format!(".{}", s(name).to_lowercase()))
                })
        })
        .collect();
    players.sort_by_key(|p| p["props"]["PlaybackStatus"] != "Playing");
    let Some(p) = players.first() else {
        if arr(&module["players"])
            .iter()
            .any(|v| s(v).eq_ignore_ascii_case("spotify"))
        {
            let mut r = item(
                "spotify-idle",
                None,
                Some("play_arrow"),
                "symbol",
                json!({}),
                "Play Spotify",
                json!({"leftClick":"player.toggle","middleClick":"player.toggle"}),
            );
            r["idle"] = json!(true);
            return vec![r];
        }
        return vec![];
    };
    let props = &p["props"];
    let meta = &props["Metadata"];
    let playing = props["PlaybackStatus"] == "Playing";
    let title = meta["xesam:title"]
        .as_str()
        .unwrap_or(s(&p["root"]["Identity"]));
    let artist = arr(&meta["xesam:artist"])
        .iter()
        .map(st)
        .collect::<Vec<_>>()
        .join(", ");
    let mut row = item(
        s(&p["service"]),
        if playing { Some(title) } else { None },
        Some(if playing { "pause" } else { "play_arrow" }),
        "symbol",
        json!({}),
        "",
        json!({"middleClick":"player.toggle","scrollUp":"player.previous","scrollDown":"player.next"}),
    );
    row["icon"]["actions"] = json!({"leftClick":"player.toggle"});
    row["idle"] = json!(!playing);
    row["artist"] = json!({"value":artist,"actions":{"leftClick":"player.artist"}});
    row["track"] = json!({"value":title,"actions":{"leftClick":"player.radio"}});
    row["native"] = (*p).clone();
    vec![row]
}
pub async fn action(action: String, row: Value, context: Value) -> Result<()> {
    if row["id"] == "spotify-idle" && action == "player.toggle" {
        return crate::navigation::play().await;
    }
    let native = row["native"].clone();
    if ["player.artist", "player.playlist", "player.radio"].contains(&action.as_str()) {
        if s(&native["service"]).starts_with("org.mpris.MediaPlayer2.spotify") {
            let meta = &native["props"]["Metadata"];
            let target = if action == "player.artist" {
                "artist"
            } else if action == "player.radio" {
                "radio"
            } else {
                "playlist"
            };
            let uri = if target == "artist" {
                meta["spotify:artistUrl"].clone()
            } else {
                meta["spotify:contextUrl"].clone()
            };
            return crate::navigation::navigate(&json!({"target":target,"player":native["service"],"trackId":crate::navigation::track_id(meta),"uri":uri})).await;
        }
    }
    ensure!(
        action != "player.radio",
        "Song radio is currently supported for Spotify"
    );
    tokio::task::spawn_blocking(move || -> Result<()> {
        let c = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let dest = s(&native["owner"]);
        ensure!(
            owner(&c, s(&native["service"]))? == dest,
            "Stale service target"
        );
        if action.starts_with("player.") {
            let p = all(&c, dest, MPATH, PLAYER)?;
            ensure!(
                p["CanControl"] == true
                    || ["player.quit", "player.artist", "player.playlist"]
                        .contains(&action.as_str()),
                "Player cannot be controlled"
            );
            if action == "player.shuffle" {
                ensure!(p["Shuffle"].is_boolean(), "Shuffle unavailable");
            }
            if ["player.quit", "player.artist", "player.playlist"].contains(&action.as_str()) {
                let root = all(&c, dest, MPATH, "org.mpris.MediaPlayer2")?;
                ensure!(
                    root[if action == "player.quit" {
                        "CanQuit"
                    } else {
                        "CanRaise"
                    }] == true,
                    "Player action unavailable"
                );
            }
            let (method, iface, args) = match action.as_str() {
                "player.toggle" => {
                    ensure!(p["CanControl"] == true, "Player cannot be controlled");
                    ensure!(
                        if p["PlaybackStatus"] == "Playing" {
                            p["CanPause"] == true
                        } else {
                            p["CanPlay"] == true
                        },
                        "Playback toggle unavailable"
                    );
                    ("PlayPause", PLAYER, ().to_variant())
                }
                "player.next" => {
                    ensure!(p["CanGoNext"] == true, "Next unavailable");
                    ("Next", PLAYER, ().to_variant())
                }
                "player.previous" => {
                    if p["CanSeek"] == true && p["Position"].as_i64().unwrap_or(0) > 1_000_000 {
                        let track = glib::variant::ObjectPath::try_from(s(
                            &p["Metadata"]["mpris:trackid"]
                        ))?;
                        ("SetPosition", PLAYER, (track, 0i64).to_variant())
                    } else {
                        ensure!(p["CanGoPrevious"] == true, "Previous unavailable");
                        ("Previous", PLAYER, ().to_variant())
                    }
                }
                "player.shuffle" => (
                    "Set",
                    PROPS,
                    (
                        PLAYER,
                        "Shuffle",
                        (!p["Shuffle"].as_bool().unwrap_or(false)).to_variant(),
                    )
                        .to_variant(),
                ),
                "player.quit" => ("Quit", "org.mpris.MediaPlayer2", ().to_variant()),
                "player.artist" | "player.playlist" => {
                    ("Raise", "org.mpris.MediaPlayer2", ().to_variant())
                }
                _ => anyhow::bail!("Unknown media action"),
            };
            call(&c, dest, MPATH, iface, method, &args)?;
        } else {
            let path = s(&native["path"]);
            let x = context["anchor"]["x"].as_f64().unwrap_or(0.) as i32;
            let y = context["anchor"]["y"].as_f64().unwrap_or(0.) as i32;
            let (method, args) = match action.as_str() {
                "tray.activate" => (
                    if native["props"]["ItemIsMenu"] == true {
                        "ContextMenu"
                    } else {
                        "Activate"
                    },
                    (x, y).to_variant(),
                ),
                "tray.secondary" => ("SecondaryActivate", (x, y).to_variant()),
                "tray.menu" => ("ContextMenu", (x, y).to_variant()),
                "tray.up" => ("Scroll", (120i32, "vertical").to_variant()),
                "tray.down" => ("Scroll", (-120i32, "vertical").to_variant()),
                _ => anyhow::bail!("Unknown tray action"),
            };
            call(&c, dest, path, SNI, method, &args)?;
        }
        Ok(())
    })
    .await?
}

fn menu_node<'a>(node: &'a Value, id: i32) -> Option<&'a Value> {
    if node[0].as_i64() == Some(id as i64) {
        return Some(node);
    }
    arr(&node[2]).iter().find_map(|n| menu_node(n, id))
}
pub async fn menu(row: Value, id: i32, activate: bool) -> Result<Value> {
    tokio::task::spawn_blocking(move || -> Result<Value> {
        let native = &row["native"];
        let c = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let dest = s(&native["owner"]);
        ensure!(owner(&c, s(&native["service"]))? == dest, "Stale tray item");
        let path = s(&native["props"]["Menu"]);
        ensure!(
            path.starts_with('/') && path != "/NO_DBUSMENU",
            "Tray menu unavailable"
        );
        let interface = "com.canonical.dbusmenu";
        if !activate {
            let _ = call(
                &c,
                dest,
                path,
                interface,
                "AboutToShow",
                &(id,).to_variant(),
            );
        }
        let layout = value(&call(
            &c,
            dest,
            path,
            interface,
            "GetLayout",
            &(0i32, -1i32, Vec::<String>::new()).to_variant(),
        )?);
        if activate {
            let node = menu_node(&layout[1], id).context("Menu item removed")?;
            ensure!(
                node[1]["enabled"] != false
                    && node[1]["visible"] != false
                    && node[1]["type"] != "separator",
                "Menu item unavailable"
            );
            call(
                &c,
                dest,
                path,
                interface,
                "Event",
                &(id, "clicked", 0i32.to_variant(), 0u32).to_variant(),
            )?;
        }
        Ok(json!({"revision":layout[0],"layout":layout[1],"owner":dest,"path":path}))
    })
    .await?
}

/// Read the authoritative clock off the GTK thread, pinned to a service owner and track.
pub async fn position(native: Value) -> Result<Value> {
    tokio::task::spawn_blocking(move || {
        let c = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let dest = s(&native["owner"]);
        ensure!(owner(&c, s(&native["service"]))? == dest, "Stale player");
        let p = all(&c, dest, MPATH, PLAYER)?;
        ensure!(p["Metadata"]["mpris:trackid"] == native["props"]["Metadata"]["mpris:trackid"], "Stale track");
        Ok(json!({"position":p["Position"],"rate":p["Rate"],"playing":p["PlaybackStatus"] == "Playing"}))
    }).await?
}

/// Read Fcitx state without starting a new CLI process or activating the daemon.
pub async fn input_method() -> Result<(String, Option<String>)> {
    tokio::task::spawn_blocking(|| {
        let c = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let read = |method| -> Result<String> {
            let reply = c.call_sync(
                Some("org.fcitx.Fcitx5"),
                "/controller",
                "org.fcitx.Fcitx.Controller1",
                method,
                Some(&().to_variant()),
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                1200,
                gio::Cancellable::NONE,
            )?;
            Ok(reply
                .child_value(0)
                .str()
                .context("Invalid input method state")?
                .to_owned())
        };
        Ok((
            read("CurrentInputMethod")?,
            read("CurrentInputMethodGroup").ok(),
        ))
    })
    .await?
}

/// Read the user unit's authoritative files through the existing session bus.
/// Used for frequent inspection; mutation still follows the policy controller.
pub async fn idle_unit() -> Result<(u32, String)> {
    tokio::task::spawn_blocking(|| {
        let c = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let path = call(
            &c,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "GetUnit",
            &("hypridle.service",).to_variant(),
        )?;
        let path = path
            .child_value(0)
            .str()
            .context("Invalid idle unit")?
            .to_owned();
        let get = |interface: &str, property: &str| -> Result<glib::Variant> {
            call(
                &c,
                "org.freedesktop.systemd1",
                &path,
                PROPS,
                "Get",
                &(interface, property).to_variant(),
            )?
            .child_value(0)
            .as_variant()
            .context("Invalid idle property")
        };
        let fragment = get("org.freedesktop.systemd1.Unit", "FragmentPath")?
            .get::<String>()
            .context("Invalid unit path")?;
        let dropins = get("org.freedesktop.systemd1.Unit", "DropInPaths")?
            .get::<Vec<String>>()
            .context("Invalid unit drop-ins")?;
        let pid = get("org.freedesktop.systemd1.Service", "MainPID")?
            .get::<u32>()
            .context("Invalid idle PID")?;
        let mut text = String::new();
        for path in std::iter::once(&fragment).chain(&dropins) {
            if path.is_empty() {
                continue;
            }
            text.push_str(&format!("# {path}\n{}\n", std::fs::read_to_string(path)?));
        }
        ensure!(!text.is_empty(), "Idle unit files unavailable");
        Ok((pid, text))
    })
    .await?
}
