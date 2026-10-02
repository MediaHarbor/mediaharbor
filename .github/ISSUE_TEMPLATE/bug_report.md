---
name: Bug report
about: Create a report to help us improve
title: "[BUG]"
labels: bug
assignees: CrossyAtom46

---

**Describe the bug**
A clear and concise description of what the bug is.

**To Reproduce**
Steps to reproduce the behavior:
1. Go to '...'
2. Click on '....'
3. Scroll down to '....'
4. See error

**Expected behavior**
A clear and concise description of what you expected to happen.

**Screenshots**
If applicable, add screenshots to help explain your problem.

**Desktop (please complete the following information):**
 - OS: [e.g. Windows 11, macOS 15, Ubuntu 24.04]
 - MediaHarbor version: [e.g. 3.0.0]
 - Install type: [installer / MSI / AppImage / deb / rpm / Flatpak / Snap / built from source]

---

## Logs

MediaHarbor writes a log file continuously while it runs. **Please attach this file** — it
is written as things happen, so it survives even if the app closes unexpectedly.

| OS | Location |
|---|---|
| Windows | `%APPDATA%\org.mediaharbor.MediaHarbor\mediaharbor.log` |
| macOS | `~/Library/Application Support/org.mediaharbor.MediaHarbor/mediaharbor.log` |
| Linux | `~/.local/share/org.mediaharbor.MediaHarbor/mediaharbor.log` |
| Linux (Flatpak) | `~/.var/app/org.mediaharbor.MediaHarbor/data/org.mediaharbor.MediaHarbor/mediaharbor.log` |
| Linux (Snap) | `~/snap/mediaharbor/current/.local/share/org.mediaharbor.MediaHarbor/mediaharbor.log` |

Paste the path into Explorer / Finder / your file manager to open the folder.

> The in-app **Logs** page shows the same messages and is handy for copying a single error,
> but it only holds what happened since the app started — if MediaHarbor closed on you,
> use the file above instead.

<details>
<summary>The app closed by itself / vanished with no error message</summary>

Attach the log file anyway — but note it will simply **stop mid-line with no error**. That is
expected for this kind of crash and is itself useful information, so please say so in the report.

**On Windows**, please also run this in PowerShell (no admin needed) and paste the output.
It reads the crash record Windows already saved:

```powershell
Get-WinEvent -FilterHashtable @{LogName='Application'; ProviderName='Windows Error Reporting'} -MaxEvents 200 -ErrorAction SilentlyContinue |
  Where-Object { $_.Message -like '*mediaharbor.exe*' } | Select-Object -First 3 | ForEach-Object {
    $p = @($_.Message -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '^P\d+:' })
    [pscustomobject]@{
      Time = $_.TimeCreated; Version = ($p[1] -replace '^P2:\s*','')
      FaultingModule = ($p[3] -replace '^P4:\s*',''); ExceptionCode = ($p[6] -replace '^P7:\s*','')
      Offset = ($p[7] -replace '^P8:\s*','')
    }
  } | Format-List
```

If it prints nothing, say so — that also tells us something.

**On macOS**: open **Console.app** → *Crash Reports*, find the newest `MediaHarbor` entry and attach it.

**On Linux**: run `coredumpctl info mediaharbor` (if `systemd-coredump` is installed) and paste the output.

</details>

**Additional context**
Add any other context about the problem here.
