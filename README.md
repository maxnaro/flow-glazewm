# GlazeWM Windows for Flow Launcher

A native Rust plugin for Flow Launcher 2.1.4 and GlazeWM 3.10.1 on Windows x64.
Search all managed workspaces by window title or process name. Select a result to
switch to its existing workspace and focus that exact window. Existing windows
are never moved. Minimized targets are restored.

## Install

Download `flow-glazewm.zip` and `Install.ps1` from the GitHub release, then run:

```powershell
.\Install.ps1 -PackagePath .\flow-glazewm.zip
```

The installer discovers Scoop's portable Flow installation. For other installations,
pass `-FlowData` with the folder containing `Settings` and `Plugins`.
It backs up Flow settings and any earlier package, sets priority to at least 2,
disables Window Walker results to avoid duplicates, and restarts a running Flow.
Use ordinary search, or `gw` to restrict results to GlazeWM windows.

If using the Windows dotfiles workspace helper, update it with
`windows/Enable-FlowWorkspace.ps1` first. Version `4-native-handoff` releases its
origin-preservation guard when this plugin selects an existing window. The plugin
also works without that helper.

To roll back, close Flow, restore `Settings.json` and any prior package from the
printed backup directory, remove this plugin's installed folder if it was new,
and restart Flow.

## Behavior

Each result carries the selected UUID and HWND. Actions revalidate that identity,
release the optional dotfiles guard, hide Flow through its API, and start a native
worker without inherited RPC pipe handles. The worker focuses the target workspace
before its window, retries once, and verifies Windows foreground focus. New user
input before activation or while awaiting focus cancels the worker. GlazeWM's own
cursor jump is accounted for during the focus commands.

Only GlazeWM-managed windows are searched. Workspace names containing whitespace,
quotes, backslashes, or a leading hyphen are not supported by this release.
Worker failures are recorded in `%LOCALAPPDATA%\flow-glazewm.log`; the log is
bounded and does not record search queries or window titles.

## Build and verify

On Windows with stable Rust:

```text
cargo test --locked
cargo clippy --locked -- -D warnings
cargo build --release --locked
```

Windows CI packages the executable, manifest, and icon. Protocol and parsing tests
also run on Linux. Desktop verification used real Flow result selection with Enter
and a mouse click across workspaces, with original window assignments checked.

## Standalone GlazeWM/Zebar startup

The release also includes `Install-GlazeStartup.ps1`. Download that single file and
run it in ordinary Windows PowerShell to start your own installed GlazeWM and Zebar
at sign-in. It does not install a dotfiles profile or change your configurations.
Use `-GlazeWMPath` and `-ZebarPath` for explicit executable paths, or `-Uninstall`
to remove its task and launcher files. Logs remain under
`%LOCALAPPDATA%\GlazeWMStartup\startup.log`.
