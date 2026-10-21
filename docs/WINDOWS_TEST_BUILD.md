# Windows test build

The Windows installer is built on a Windows GitHub Actions runner. From the repository page, open **Actions → Build Windows installer → Run workflow**. When the run finishes, download the `AD3D-windows-x64-installer` artifact. It contains the NSIS setup executable to share with the tester.

The workflow also builds automatically when changes are pushed to `main`. Artifacts are available for 14 days.

This is an unsigned x64 test build. Windows may show a SmartScreen prompt because the app has no publisher signature. The installer uses Tauri's default WebView2 bootstrapper behavior; the tester needs internet access if WebView2 is not already installed. Windows 10 1803 and later and Windows 11 generally include WebView2.

The repo needs to be pushed to GitHub before the workflow can run. The app currently has no Git remote configured in this workspace.
