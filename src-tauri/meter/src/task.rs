//! 登录后以最高权限启动，这样开机不用再弹一次 UAC。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;
pub const TASK_NAME: &str = "NidongMeter";

pub fn install(exe: &Path) -> Result<(), String> {
    let text = exe.to_string_lossy().to_ascii_lowercase();
    if text.contains("\\deps\\") {
        return Err("测试程序不登记开机启动".into());
    }
    let sid = current_sid().ok_or_else(|| "读不到当前用户".to_string())?;
    let xml = task_xml(&exe.display().to_string(), &sid);
    let path = std::env::temp_dir().join("NidongMeter-task.xml");
    write_utf16(&path, &xml).map_err(|err| err.to_string())?;
    let result = run(&["/Create", "/F", "/TN", TASK_NAME, "/XML", &path.display().to_string()]);
    let _ = fs::remove_file(&path);
    result.map(|_| ())
}

pub fn remove() -> Result<(), String> {
    if run(&["/Query", "/TN", TASK_NAME]).is_err() {
        return Ok(());
    }
    run(&["/Delete", "/TN", TASK_NAME, "/F"]).map(|_| ())
}

pub fn kick_if_idle(enabled: bool, running: bool) {
    if !enabled || running {
        return;
    }
    static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if last.map(|t| t.elapsed() < Duration::from_secs(60)).unwrap_or(false) {
        return;
    }
    *last = Some(Instant::now());
    let _ = Command::new("schtasks")
        .args(["/Run", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

pub fn task_xml(exe: &str, sid: &str) -> String {
    let exe = xml_escape(exe);
    let sid = xml_escape(sid);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Principals>
    <Principal id="Author">
      <UserId>{sid}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{sid}</UserId>
    </LogonTrigger>
  </Triggers>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--run</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn write_utf16(path: &Path, text: &str) -> std::io::Result<()> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(path, bytes)
}

fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|err| err.to_string())?;
    let mut text = String::new();
    text.push_str(&decode_console(&output.stdout));
    text.push_str(&decode_console(&output.stderr));
    if output.status.success() {
        Ok(text)
    } else {
        let text = text.trim().to_string();
        if text.is_empty() {
            Err(format!("计划任务没有建成，代码 {}", output.status))
        } else {
            Err(text)
        }
    }
}

fn decode_console(bytes: &[u8]) -> String {
    if std::str::from_utf8(bytes).is_ok() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    text.into_owned()
}

fn current_sid() -> Option<String> {
    let output = Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = decode_console(&output.stdout);
    take_sid(&text).map(str::to_string)
}

fn take_sid(text: &str) -> Option<&str> {
    let start = text.find("S-1-")?;
    let tail = &text[start..];
    let end = tail
        .find(|c: char| !(c.is_ascii_digit() || c == '-' || c == 'S'))
        .unwrap_or(tail.len());
    let sid = &tail[..end];
    if sid.len() > 8 { Some(sid) } else { None }
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("nidong-meter.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_runs_hidden_at_logon_with_highest_rights() {
        let xml = task_xml(r"C:\Program Files\泥洞\nidong-meter.exe", "S-1-5-21-1-2-3-1001");
        assert!(xml.contains(r"C:\Program Files\泥洞\nidong-meter.exe"));
        assert!(xml.contains("--run"));
        assert!(xml.contains("HighestAvailable"));
        assert!(xml.contains("InteractiveToken"));
        assert!(xml.contains("LogonTrigger"));
        assert!(xml.contains("PT0S"));
    }

    #[test]
    fn take_sid_reads_the_csv_cell() {
        assert_eq!(take_sid("\"PC\\asus\",\"S-1-5-21-9-8-7-1001\"\r\n"), Some("S-1-5-21-9-8-7-1001"));
    }
}
