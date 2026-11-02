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
    request_at(command, "127.0.0.1:6123")
}
fn request_at(command: &str, address: &str) -> Result<Value, Error> {
    let stream = TcpStream::connect_timeout(&address.parse()?, Duration::from_secs(2))?;
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
fn flow_foreground() -> u64 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;
        let handle = foreground();
        let hwnd = handle as *mut std::ffi::c_void;
        if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) } == 0 {
            return 0;
        }
        let mut cloaked = 0u32;
        unsafe {
            windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(
                hwnd,
                14,
                &mut cloaked as *mut u32 as *mut std::ffi::c_void,
                4,
            );
        }
        if cloaked != 0 {
            return 0;
        }
        let mut title = [0u16; 256];
        let length = unsafe {
            GetWindowTextW(
                handle as *mut std::ffi::c_void,
                title.as_mut_ptr(),
                title.len() as i32,
            )
        };
        if length > 0 && String::from_utf16_lossy(&title[..length as usize]) == "Flow.Launcher" {
            return handle;
        }
    }
    0
}
#[cfg(windows)]
fn release_guard() -> Result<(), Error> {
    use std::io::{Read, Write};
    use std::os::windows::io::AsRawHandle;
    let mut session = 0;
    unsafe {
        windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(
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
            let mode = windows_sys::Win32::System::Pipes::PIPE_NOWAIT;
            if unsafe {
                windows_sys::Win32::System::Pipes::SetNamedPipeHandleState(
                    pipe.as_raw_handle(),
                    &mode,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            pipe.write_all(b"release\n")?;
            let mut reply = [0; 3];
            let until = Instant::now() + Duration::from_secs(3);
            let mut received = 0;
            while received < reply.len() {
                match pipe.read(&mut reply[received..]) {
                    Ok(0) => {
                        if Instant::now() >= until {
                            return Err("Workspace helper timed out".into());
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(n) => received += n,
                    Err(e) if e.raw_os_error() == Some(232) => {
                        if Instant::now() >= until {
                            return Err("Workspace helper timed out".into());
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
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
#[cfg(windows)]
fn last_input() -> u32 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        GetLastInputInfo(&mut info);
    }
    info.dwTime
}
#[cfg(not(windows))]
fn last_input() -> u32 {
    0
}
fn focus(id: &str, handle: u64, input_time: u32) -> Result<(), Error> {
    let id = uuid::Uuid::parse_str(id)?.to_string();
    for _ in 0..2 {
        if last_input() != input_time {
            return Err("Focus cancelled by new user input".into());
        }
        let all = snapshot()?;
        let target = all
            .iter()
            .find(|w| w["id"] == id && w["handle"].as_u64() == Some(handle))
            .ok_or("Selected window has closed")?;
        let workspace = target["workspace"].as_str().ok_or("Missing workspace")?;
        if workspace.starts_with('-')
            || workspace.chars().any(char::is_whitespace)
            || workspace.contains(['"', '\\'])
        {
            return Err("Unsupported workspace name".into());
        }
        request(&format!("command focus --workspace {workspace}"))?;
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
            if last_input() != input_time {
                return Err("Focus cancelled by new user input".into());
            }
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
        let previous = args.get(4).ok_or("Missing Flow HWND")?.parse::<u64>()?;
        let until = Instant::now() + Duration::from_secs(2);
        while previous != 0 && flow_foreground() == previous && Instant::now() < until {
            thread::sleep(Duration::from_millis(20));
        }
        if previous != 0 && flow_foreground() == previous {
            return Err("Flow did not hide before focus".into());
        }
        focus(
            args.get(2).ok_or("Missing target UUID")?,
            args.get(3).ok_or("Missing HWND")?.parse()?,
            args.get(5).ok_or("Missing input timestamp")?.parse()?,
        )?;
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
            let mut worker = Command::new(env::current_exe()?);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                worker.creation_flags(0x08000000);
            }
            worker
                .args([
                    "--focus",
                    parameters[0].as_str().ok_or("Missing id")?,
                    &parameters[1].as_u64().ok_or("Missing handle")?.to_string(),
                    &flow_foreground().to_string(),
                    &last_input().to_string(),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(json!({"method":"Flow.Launcher.HideMainWindow","parameters":[]}))
        }
        "context_menu" => Ok(json!({"result":[]})),
        _ => Err("Unknown request".into()),
    }
}
fn main() {
    if env::args().nth(1).as_deref() == Some("--focus") {
        if let Err(e) = run() {
            if let Some(root) = env::var_os("LOCALAPPDATA") {
                use std::io::Write;
                let path = std::path::PathBuf::from(root).join("flow-glazewm.log");
                if std::fs::metadata(&path)
                    .map(|m| m.len() > 262144)
                    .unwrap_or(false)
                {
                    let _ = std::fs::remove_file(&path);
                }
                if let Ok(mut log) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(log, "focus failed: {e}");
                }
            }
            std::process::exit(1);
        }
        return;
    }
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
    fn fake_reply(reply: Value) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(socket).unwrap();
            assert_eq!(ws.read().unwrap().to_text().unwrap(), "query workspaces");
            ws.send(tungstenite::Message::Text(reply.to_string().into()))
                .unwrap();
        });
        address
    }
    #[test]
    fn ipc_success() {
        let address = fake_reply(json!({"success":true,"data":{"workspaces":[]}}));
        assert!(request_at("query workspaces", &address).is_ok());
    }
    #[test]
    fn ipc_rejection() {
        let address = fake_reply(json!({"success":false,"error":"fixture rejection"}));
        assert!(request_at("query workspaces", &address)
            .unwrap_err()
            .to_string()
            .contains("fixture rejection"));
    }
    #[test]
    fn ipc_disconnect() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(request_at("query workspaces", &address).is_err());
    }
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
