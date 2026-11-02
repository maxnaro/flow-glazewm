use serde_json::{json, Value};
use std::{
    env,
    net::TcpStream,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
type Error = Box<dyn std::error::Error>;
fn request(command: &str) -> Result<Value, Error> {
    let stream = TcpStream::connect_timeout(&"127.0.0.1:6123".parse()?, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let (mut ws, _) = tungstenite::client("ws://127.0.0.1:6123", stream)?;
    ws.send(tungstenite::Message::Text(command.into()))?;
    loop {
        let message = ws.read()?;
        if message.is_text() {
            let v: Value = serde_json::from_str(message.to_text()?)?;
            if v["success"] != true {
                return Err(format!("GlazeWM rejected request: {}", v["error"]).into());
            }
            return Ok(v);
        }
    }
}
fn windows(nodes: &[Value], workspace: &str, out: &mut Vec<Value>) {
    for node in nodes {
        if node["type"] == "window" {
            let mut window = node.clone();
            window["workspace"] = json!(workspace);
            out.push(window);
        }
        if let Some(children) = node["children"].as_array() {
            windows(children, workspace, out);
        }
    }
}
fn snapshot() -> Result<Vec<Value>, Error> {
    let response = request("query workspaces")?;
    let mut out = Vec::new();
    for ws in response["data"]["workspaces"]
        .as_array()
        .ok_or("Missing workspaces")?
    {
        windows(
            ws["children"].as_array().ok_or("Missing children")?,
            ws["name"].as_str().ok_or("Missing workspace name")?,
            &mut out,
        );
    }
    Ok(out)
}
fn matches(query: &str, text: &str) -> bool {
    let lower = text.to_lowercase();
    let mut chars = lower.chars();
    query
        .to_lowercase()
        .chars()
        .all(|c| chars.by_ref().any(|x| x == c))
}
#[cfg(windows)]
fn foreground() -> u64 {
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as u64 }
}
#[cfg(not(windows))]
fn foreground() -> u64 {
    0
}
#[cfg(windows)]
fn release_guard() -> Result<(), Error> {
    use std::io::{Read, Write};
    let mut session = 0;
    unsafe {
        windows_sys::Win32::System::Threading::ProcessIdToSessionId(
            std::process::id(),
            &mut session,
        );
    }
    let path = format!(r"\\.\pipe\DotfilesFlowWorkspaceControl-{session}");
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(mut pipe) => {
            pipe.write_all(b"release\n")?;
            let mut reply = [0; 3];
            pipe.read_exact(&mut reply)?;
            if &reply != b"OK\n" {
                return Err("Workspace helper rejected handoff".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
#[cfg(not(windows))]
fn release_guard() -> Result<(), Error> {
    Ok(())
}
fn focus(id: &str, handle: u64) -> Result<(), Error> {
    let id = uuid::Uuid::parse_str(id)?.to_string();
    release_guard()?;
    for _ in 0..2 {
        let all = snapshot()?;
        let target = all
            .iter()
            .find(|w| w["id"] == id && w["handle"].as_u64() == Some(handle))
            .ok_or("Selected window has closed")?;
        let workspace = target["workspace"].as_str().ok_or("Missing workspace")?;
        if workspace.contains(['"', '\\', '\n', '\r']) {
            return Err("Unsupported workspace name".into());
        }
        request(&format!("command focus --workspace \"{workspace}\""))?;
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let hwnd = handle as *mut std::ffi::c_void;
            if IsIconic(hwnd) != 0 {
                ShowWindowAsync(hwnd, SW_RESTORE);
            }
        }
        request(&format!("command focus --container-id {id}"))?;
        let until = Instant::now() + Duration::from_secs(1);
        while Instant::now() < until {
            if foreground() == handle {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    Err("Windows did not grant foreground focus".into())
}
fn run() -> Result<Value, Error> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("--focus") {
        // Flow hides only after its executable action exits. Wait for that handoff.
        let previous = args[4].parse::<u64>()?;
        let until = Instant::now() + Duration::from_secs(2);
        while foreground() == previous && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        focus(&args[2], args[3].parse()?)?;
        return Ok(json!({}));
    }
    let input: Value = serde_json::from_str(args.get(1).ok_or("Expected Flow JSON-RPC request")?)?;
    let parameters = &input["parameters"];
    match input["method"].as_str().unwrap_or("") {
        "query" => {
            let query = parameters[0].as_str().unwrap_or("");
            let mut results = Vec::new();
            for w in snapshot()? {
                let title = w["title"].as_str().unwrap_or("");
                let process = w["processName"].as_str().unwrap_or("");
                if title.is_empty() || !matches(query, &format!("{title} {process}")) {
                    continue;
                }
                results.push(json!({"Title":title,"SubTitle":format!("Running: {process} • Workspace {}",w["workspace"].as_str().unwrap_or("")),"Score":100,"IcoPath":"icon.png","JsonRPCAction":{"method":"focus","parameters":[w["id"],w["handle"]]}}));
            }
            Ok(json!({"result":results}))
        }
        "focus" => {
            release_guard()?;
            Command::new(env::current_exe()?)
                .args([
                    "--focus",
                    parameters[0].as_str().ok_or("Missing id")?,
                    &parameters[1].as_u64().ok_or("Missing handle")?.to_string(),
                    &foreground().to_string(),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(json!({"method":"Flow.Launcher.HideApp","parameters":[]}))
        }
        "context_menu" => Ok(json!({"result":[]})),
        _ => Err("Unknown request".into()),
    }
}
fn main() {
    match run() {
        Ok(v) => println!("{v}"),
        Err(e) => println!(
            "{}",
            json!({"result":[],"method":"Flow.Launcher.ShowMsg","parameters":["GlazeWM window focus failed",e.to_string(),""]})
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy() {
        assert!(matches("outlook", "Inbox - Outlook olk"));
        assert!(!matches("outlook", "Firefox"));
    }
    #[test]
    fn nested() {
        let mut out = Vec::new();
        windows(
            &[json!({"type":"split","children":[{"type":"window","id":"x"}]})],
            "1",
            &mut out,
        );
        assert_eq!(out[0]["workspace"], "1");
    }
}
