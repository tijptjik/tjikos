//! Exercise the installed-command contract using only synthetic, offline state.
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use whatsover_engine::{Claim, Journal, SendState, TextRequest, native::NativeAccount};

fn cli(dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_whatsover"));
    command.arg("--state-dir").arg(dir);
    command
}

#[tokio::test]
async fn image_replay_uses_content_identity_and_private_persisted_attachment() -> anyhow::Result<()>
{
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir_in("/tmp")?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let file = dir.path().join("synthetic.jpg");
    let bytes = [0xff, 0xd8, 0xff, 0x00];
    std::fs::write(&file, bytes)?;
    let hash = hex::encode(Sha256::digest(bytes));
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    );
    let account = NativeAccount::open(dir.path()).await?;
    account
        .journal
        .stage_attachment(id.clone(), "synthetic-descriptor".into())
        .await?;
    let request = TextRequest {
        to: "+15555550100".into(),
        text: String::new(),
        request_id: "synthetic-image".into(),
        attachment_id: Some(id.clone()),
        reply_to: None,
        reaction: None,
    };
    let Claim::New(mut record) = account.journal.claim(request).await? else {
        panic!("new request")
    };
    record.state = SendState::Submitted;
    account
        .journal
        .record_delivery_events(&[serde_json::json!({
            "kind":"server_ack", "data":{"id":record.message_id,"class":"message","error":null}
        })])
        .await?;
    account.journal.finish(record).await?;
    drop(account);
    let account = NativeAccount::open(dir.path()).await?;
    assert_eq!(
        account.journal.attachment(&id).await?.as_deref(),
        Some("synthetic-descriptor")
    );
    drop(account);
    let invoke = || {
        cli(dir.path())
            .args([
                "send",
                "--to",
                "+15555550100",
                "--id",
                "synthetic-image",
                "--image",
            ])
            .arg(&file)
            .output()
    };
    assert!(invoke()?.status.success());
    std::fs::write(&file, [0xff, 0xd8, 0xff, 0x01])?;
    let changed = invoke()?;
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("different message"));
    Ok(())
}

#[tokio::test]
async fn automation_replays_offline_and_never_reports_uncertainty_as_success() -> anyhow::Result<()>
{
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir_in("/tmp")?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let account = NativeAccount::open(dir.path()).await?;
    let request = TextRequest {
        request_id: "synthetic-job-1".into(),
        to: "+15555550100".into(),
        text: "Hello\n世界\n".into(),
        reply_to: None,
        reaction: None,
        attachment_id: None,
    };
    let Claim::New(mut record) = account.journal.claim(request.clone()).await? else {
        panic!("new request")
    };
    record.state = SendState::Submitted;
    account
        .journal
        .record_delivery_events(&[serde_json::json!({
            "kind":"server_ack", "data":{"id":record.message_id,"class":"message","error":null}
        })])
        .await?;
    account.journal.finish(record).await?;
    let legacy = TextRequest {
        request_id: "synthetic-no-ack".into(),
        ..request.clone()
    };
    let Claim::New(mut legacy_record) = account.journal.claim(legacy).await? else {
        panic!("new request")
    };
    legacy_record.state = SendState::Submitted;
    account.journal.finish(legacy_record).await?;
    let pending = TextRequest {
        request_id: "synthetic-interrupted".into(),
        ..request.clone()
    };
    account.journal.claim(pending).await?;
    drop(account);

    let status = cli(dir.path()).arg("status").output()?;
    assert!(status.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status.stdout)?["paired"],
        false
    );
    let check = cli(dir.path()).arg("check").output()?;
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("no linked Rust session"));

    for (id, success, state) in [
        ("synthetic-job-1", true, "submitted"),
        ("synthetic-interrupted", false, "uncertain"),
        ("synthetic-no-ack", false, "submitted"),
    ] {
        let mut child = cli(dir.path())
            .args(["send", "--to", &request.to, "--stdin", "--id", id])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.text.as_bytes())?;
        let output = child.wait_with_output()?;
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout)?["state"],
            state
        );
    }
    let delivery = cli(dir.path())
        .args([
            "send",
            "--to",
            &request.to,
            "--text",
            &request.text,
            "--id",
            "synthetic-job-1",
            "--wait-delivery",
        ])
        .output()?;
    assert!(!delivery.status.success());
    assert!(
        String::from_utf8_lossy(&delivery.stderr).contains("recipient delivery is not confirmed")
    );
    let result = cli(dir.path())
        .args(["result", "synthetic-job-1"])
        .output()?;
    let result: serde_json::Value = serde_json::from_slice(&result.stdout)?;
    assert_eq!(result["transport"]["server"], "accepted");
    assert_eq!(result["transport"]["recipientDeliveryConfirmed"], false);
    let conflict = cli(dir.path())
        .args([
            "send",
            "--to",
            &request.to,
            "--text",
            "changed",
            "--id",
            &request.request_id,
        ])
        .output()?;
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("different message"));
    let fresh = cli(dir.path())
        .args([
            "send",
            "--to",
            &request.to,
            "--text",
            "synthetic",
            "--id",
            "fresh",
        ])
        .output()?;
    assert!(!fresh.status.success());
    assert!(String::from_utf8_lossy(&fresh.stderr).contains("no linked Rust session"));
    let missing = cli(dir.path()).args(["result", "fresh"]).output()?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&missing.stdout)?,
        serde_json::Value::Null
    );
    Ok(())
}
