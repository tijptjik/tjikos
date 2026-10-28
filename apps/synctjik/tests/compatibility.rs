use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    process::Command,
};
use synctjik::{engine::Engine, fs as tree, merge, util::*};
fn policy(remote: &Path) -> Value {
    json!({"version":2,"master":"leader","remote":remote,"identity":"~/.keys/test","recipient":"unused","initial_sync_required":[],"folders":{"assets":{"mode":"sync","encrypted":false,"retention":"daily-weekly-monthly"}}})
}
fn engine(config: Value, base: &Path, machine: &str) -> Engine {
    let home = base.join(machine);
    fs::create_dir_all(&home).unwrap();
    Engine::new(
        config,
        home,
        machine.into(),
        base.join(format!("state-{machine}")),
    )
    .unwrap()
}
fn python(script: &str, args: &[&Path]) -> String {
    let out = Command::new("python3")
        .arg("-B")
        .arg("-c")
        .arg(script)
        .args(args)
        .env(
            "PYTHONPATH",
            format!("{}/tests", env!("CARGO_MANIFEST_DIR")),
        )
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
#[test]
fn python_reads_rust_and_rust_reads_python_snapshots() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = policy(&tmp.path().join("store"));
    let a = engine(cfg.clone(), tmp.path(), "leader");
    fs::create_dir(a.home.join("assets")).unwrap();
    fs::write(a.home.join("assets/hello"), "hello 世界").unwrap();
    fs::set_permissions(
        a.home.join("assets/hello"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    symlink("hello", a.home.join("assets/link")).unwrap();
    let r = a.backup("assets", false, &mut |_, _| {}).unwrap();
    assert_eq!(r["version"], 2);
    let config = tmp.path().join("policy.json");
    atomic_json(&config, &cfg).unwrap();
    let dst = tmp.path().join("python-restored");
    python("import sys,json\nfrom pathlib import Path\nfrom legacy.core import Archives\np,h,d=map(Path,sys.argv[1:]); a=Archives(json.loads(p.read_text()),home=h,machine='leader',state=h/'python-state');a.restore('assets','leader',d);(h/'assets'/'hello').write_text('python version');a.backup('assets',force=True)",&[&config,&a.home,&dst]);
    assert_eq!(fs::read_to_string(dst.join("hello")).unwrap(), "hello 世界");
    let restored = tmp.path().join("rust-restored");
    a.restore("assets", "leader", &restored, None).unwrap();
    assert_eq!(
        fs::read_to_string(restored.join("hello")).unwrap(),
        "python version"
    );
}
#[test]
fn merge_matches_python_including_unicode_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    let e = |h: &str| json!({"type":"file","mode":384,"size":1,"sha256":h.repeat(64)});
    let cases = json!([[{}, {"é":e("a")},{"é":e("b")}],[{"a":e("a")},{},{"a":e("b")}],[{"d":{"type":"dir","mode":448},"d/a":e("a")},{"d":{"type":"dir","mode":448},"d/new":e("b")},{}]]);
    let path = tmp.path().join("cases");
    atomic_json(&path, &cases).unwrap();
    let oracle:Value=serde_json::from_str(&python("import sys,json\nfrom legacy.core import merge_entries,digest\nc=json.load(open(sys.argv[1]));print(json.dumps({'merge':[merge_entries(*v) for v in c],'hash':digest('资产')}))",&[&path])).unwrap();
    for (i, c) in cases.as_array().unwrap().iter().enumerate() {
        let (d, ch, co) = merge::merge(
            &merge::entries(&c[0]).unwrap(),
            &merge::entries(&c[1]).unwrap(),
            &merge::entries(&c[2]).unwrap(),
            false,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(json!([d, ch, co]), oracle["merge"][i]);
    }
    assert_eq!(digest(&json!("资产")), oracle["hash"]);
}
#[test]
fn apply_preserves_exclusions_rejects_stale_plans_and_keeps_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = policy(&tmp.path().join("store"));
    cfg["folders"]["assets"]["exclude"] = json!(["cache"]);
    let a = engine(cfg.clone(), tmp.path(), "leader");
    let b = engine(cfg, tmp.path(), "laptop");
    fs::create_dir(a.home.join("assets")).unwrap();
    fs::write(a.home.join("assets/hello"), "first").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    let (p, _) = b.plan("assets", "leader", None).unwrap();
    b.apply(&p, &BTreeMap::new()).unwrap();
    fs::write(b.home.join("assets/cache"), "local cache").unwrap();
    fs::write(a.home.join("assets/hello"), "second").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    let (p, _) = b.plan("assets", "leader", None).unwrap();
    fs::write(b.home.join("assets/hello"), "local change").unwrap();
    assert!(b.apply(&p, &BTreeMap::new()).is_err());
    let (p, _) = b.plan("assets", "leader", None).unwrap();
    let r = b
        .apply(&p, &BTreeMap::from([("hello".into(), "source".into())]))
        .unwrap();
    assert_eq!(
        fs::read_to_string(b.home.join("assets/cache")).unwrap(),
        "local cache"
    );
    assert_eq!(
        fs::read_to_string(Path::new(r["recovery"].as_str().unwrap()).join("hello")).unwrap(),
        "local change"
    );
    assert_eq!(
        fs::read_to_string(b.home.join("assets/hello")).unwrap(),
        "second"
    );
}
#[test]
fn missing_remote_archive_forces_republication_and_tamper_blocks_restore() {
    let tmp = tempfile::tempdir().unwrap();
    let a = engine(policy(&tmp.path().join("store")), tmp.path(), "leader");
    fs::create_dir(a.home.join("assets")).unwrap();
    fs::write(a.home.join("assets/a"), "hello").unwrap();
    let first = a.backup("assets", false, &mut |_, _| {}).unwrap();
    assert_eq!(
        a.backup("assets", false, &mut |_, _| {}).unwrap()["skipped"],
        true
    );
    let path = tmp
        .path()
        .join("store/leader/assets")
        .join(text(&first, "snapshot").unwrap())
        .join("tree.tar.zst");
    fs::remove_file(path).unwrap();
    let second = a.backup("assets", false, &mut |_, _| {}).unwrap();
    assert_ne!(first["snapshot"], second["snapshot"]);
    let path = tmp
        .path()
        .join("store/leader/assets")
        .join(text(&second, "snapshot").unwrap())
        .join("tree.tar.zst");
    fs::write(path, "tamper").unwrap();
    let dest = tmp.path().join("restore");
    assert!(a.restore("assets", "leader", &dest, None).is_err());
    assert!(!dest.exists());
}
#[test]
fn encrypted_roundtrip_and_sqlite_wal_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = policy(&tmp.path().join("store"));
    let key = tmp.path().join("key");
    assert!(Command::new("age-keygen")
        .arg("-o")
        .arg(&key)
        .output()
        .unwrap()
        .status
        .success());
    let pubkey = Command::new("age-keygen")
        .arg("-y")
        .arg(&key)
        .output()
        .unwrap();
    cfg["identity"] = json!(key);
    cfg["recipient"] = json!(String::from_utf8(pubkey.stdout).unwrap().trim());
    cfg["folders"]["assets"]["encrypted"] = json!(true);
    cfg["folders"]["assets"]["sqlite"] = json!(["state.db"]);
    let a = engine(cfg, tmp.path(), "leader");
    fs::create_dir(a.home.join("assets")).unwrap();
    let db = rusqlite::Connection::open(a.home.join("assets/state.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE state (v TEXT); INSERT INTO state VALUES ('committed WAL');").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    let dest = tmp.path().join("restore");
    a.restore("assets", "leader", &dest, None).unwrap();
    let recovered = rusqlite::Connection::open(dest.join("state.db")).unwrap();
    let v: String = recovered
        .query_row("SELECT v FROM state", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "committed WAL");
    assert!(!dest.join("state.db-wal").exists());
}
#[test]
fn symlink_parent_and_manifest_traversal_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    symlink("/tmp", tmp.path().join("outside")).unwrap();
    assert!(tree::safe_root(tmp.path(), "outside/secret").is_err());
    for name in ["../x", "/x", "a/../x"] {
        assert!(tree::check_tree(&BTreeMap::from([(
            name.into(),
            json!({"type":"dir","mode":448})
        )]))
        .is_err());
    }
    assert!(tree::check_tree(&BTreeMap::from([
        (
            "a".into(),
            json!({"type":"link","mode":511,"target":"/tmp"})
        ),
        ("a/b".into(), json!({"type":"dir","mode":448}))
    ]))
    .is_err());
}
#[test]
fn lock_is_shared_with_legacy_process() {
    let tmp = tempfile::tempdir().unwrap();
    let a = engine(policy(&tmp.path().join("store")), tmp.path(), "leader");
    let _lock = a.lock().unwrap();
    let out=python("import fcntl,sys\nf=open(sys.argv[1]+'/lock','a')\ntry: fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB); print('unlocked')\nexcept BlockingIOError: print('locked')",&[&a.state]);
    assert_eq!(out.trim(), "locked");
}
#[test]
fn telegram_compaction_matches_existing_recovery_bytes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("telegram");
    let oracle:Value=serde_json::from_str(&python("import sys,json,hashlib\nfrom pathlib import Path\nfrom legacy.telegram_fixtures import write_fixture\nfrom legacy.telegram import theme_recovery\np=Path(sys.argv[1]);write_fixture(p,cached_background=b'cached pixels'*10000);h,c=theme_recovery(p);print(json.dumps({'hashes':h,'compacted':{n:hashlib.sha256(v).hexdigest() for n,v in c.items()}}))",&[&root])).unwrap();
    let (hashes, contents) = synctjik::telegram::recovery(&root).unwrap();
    assert_eq!(json!(hashes), oracle["hashes"]);
    for (n, v) in contents {
        let p = tmp.path().join("compact");
        fs::write(&p, v).unwrap();
        assert_eq!(hash(&p).unwrap(), oracle["compacted"][n]);
    }
}
#[test]
fn active_writer_prevents_apply() {
    let tmp = tempfile::tempdir().unwrap();
    let a = engine(policy(&tmp.path().join("store")), tmp.path(), "leader");
    let b = engine(a.config.clone(), tmp.path(), "client");
    fs::create_dir(a.home.join("assets")).unwrap();
    fs::write(a.home.join("assets/a"), "source").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    fs::create_dir(b.home.join("assets")).unwrap();
    fs::write(b.home.join("assets/a"), "local").unwrap();
    let (plan, _) = b.plan("assets", "leader", None).unwrap();
    let mut child = Command::new("python3")
        .args([
            "-c",
            "import sys,time; f=open(sys.argv[1],'a'); print('ready',flush=True); time.sleep(30)",
        ])
        .arg(b.home.join("assets/a"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::BufRead;
    let mut ready = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    let result = b.apply(&plan, &BTreeMap::from([("a".into(), "source".into())]));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(result.unwrap_err().to_string().contains("writing"));
    assert_eq!(
        fs::read_to_string(b.home.join("assets/a")).unwrap(),
        "local"
    );
}
#[test]
fn sqlite_recovery_omits_only_snapshot_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("source.db");
    let dst = tmp.path().join("recovery.db");
    let db = rusqlite::Connection::open(&src).unwrap();
    db.execute_batch("CREATE TABLE history (v TEXT); CREATE TABLE config (v TEXT); INSERT INTO history VALUES('large history'); INSERT INTO config VALUES('credentials');").unwrap();
    tree::sqlite_copy(
        &src,
        &dst,
        Some(&json!({"omit_tables":["history"],"max_bytes":1048576})),
    )
    .unwrap();
    let recovered = rusqlite::Connection::open(dst).unwrap();
    assert_eq!(
        recovered
            .query_row("SELECT count(*) FROM history", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM history", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        recovered
            .query_row("SELECT v FROM config", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "credentials"
    );
}
#[test]
fn python_and_rust_apply_each_others_plans_and_baselines() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = policy(&tmp.path().join("store"));
    let a = engine(cfg.clone(), tmp.path(), "leader");
    let b = engine(cfg.clone(), tmp.path(), "client");
    fs::create_dir(a.home.join("assets")).unwrap();
    fs::write(a.home.join("assets/hello"), "first").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    let config = tmp.path().join("policy.json");
    atomic_json(&config, &cfg).unwrap();
    let plan=python("import json,sys\nfrom pathlib import Path\nfrom legacy.core import Archives\nc,h,s=map(Path,sys.argv[1:]);a=Archives(json.loads(c.read_text()),home=h,machine='client',state=s);print(a.plan('assets','leader')[0])",&[&config,&b.home,&b.state]);
    b.apply(Path::new(plan.trim()), &BTreeMap::new()).unwrap();
    fs::write(a.home.join("assets/hello"), "second").unwrap();
    a.backup("assets", false, &mut |_, _| {}).unwrap();
    let (plan, _) = b.plan("assets", "leader", None).unwrap();
    python("import json,sys\nfrom pathlib import Path\nfrom legacy.core import Archives\nc,h,s,p=map(Path,sys.argv[1:]);a=Archives(json.loads(c.read_text()),home=h,machine='client',state=s);a.apply(p)",&[&config,&b.home,&b.state,&plan]);
    assert_eq!(
        fs::read_to_string(b.home.join("assets/hello")).unwrap(),
        "second"
    );
    let (_, plan) = b.plan("assets", "leader", None).unwrap();
    assert!(plan["changes"].as_array().unwrap().is_empty());
}
#[test]
fn malicious_archive_member_never_escapes_destination() {
    use std::io::Write;
    let tmp = tempfile::tempdir().unwrap();
    let archive = tmp.path().join("bad.zst");
    let mut enc = zstd::Encoder::new(fs::File::create(&archive).unwrap(), 1).unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o600);
    header.set_size(4);
    header.set_entry_type(tar::EntryType::Regular);
    let name = b"../outside";
    header.as_mut_bytes()[..name.len()].copy_from_slice(name);
    header.set_cksum();
    enc.write_all(header.as_bytes()).unwrap();
    enc.write_all(b"evil").unwrap();
    enc.write_all(&vec![0; 508 + 1024]).unwrap();
    enc.finish().unwrap();
    assert!(
        synctjik::archive::unpack(&archive, &BTreeMap::new(), &tmp.path().join("dest")).is_err()
    );
    assert!(!tmp.path().join("outside").exists());
}
