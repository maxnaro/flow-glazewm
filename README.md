# GlazeWM Windows for Flow Launcher

Native Rust executable plugin for Flow Launcher 2.1.4 and GlazeWM 3.10.1.
Searches titles and process names across all managed workspaces. Selection switches
workspace before focusing the selected UUID/HWND; it never moves existing windows.

Build on Windows with stable Rust: `cargo test --locked` then `cargo build --release --locked`.
Copy the executable, plugin.json and icon.png into a dedicated Flow UserData/Plugins folder.
Restart Flow and give GlazeWM Windows priority 2. Disable Window Walker to avoid duplicates.

**Development status:** not ready for activation. Dotfiles helper handoff, bounded
pipe reads, action error reporting, user cancellation, and live focus validation
are still pending. Do not disable Window Walker until those checks are complete.
