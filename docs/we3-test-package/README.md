# WE-3 Cross-Network Test Package

This package prepares a two-laptop test of the current native-assisted peer
connection. It is a test procedure, not a packaged game distribution. No
cross-network run has been performed yet.

## Safety and test boundaries

- Use only the Linux home-broadband laptop and the Windows test laptop (the
  Windows 11 phone-hotspot laptop for this run), with no VPN, tunnel service,
  or third-party relay tunnel.
- Each laptop runs its own native gateway and browser client. A browser talks
  only to the gateway on the same laptop. The game's ticket connects the peers.
- Do not disable the firewall, add or edit firewall rules, change
  authentication, or bypass a publisher warning for an unexpected executable.
  Use the firewall state already present and record it on every attempt.
- The Windows feasibility check already created Public-profile inbound allow
  rules for the gateway program. Leave those rules unchanged. If a new firewall
  prompt appears, choose **Cancel** rather than creating another allow rule.
  If that prevents a connection, record the result and stop for PM direction.
- Do not include tickets, node IDs, or private key material in shared sheets,
  screenshots, or diagnostic logs. Tickets are short-lived join invitations.
  Move the current ticket between laptops only through the tester's shared
  text pad. The pad name must be long and random, and its address must be known
  only to the tester. Never share or write the address in any repository file.
  Keep only the current ticket in the pad; clear it after each attempt and at
  the end of each session. Never use the pad for log files.
- Use the same repository commit on both laptops. Build native code on each
  operating system; do not copy a Linux executable to Windows or create a
  packaged distribution.

## Source revision and prerequisites

At the start of a test batch, record the commit from both machines:

```text
git rev-parse HEAD
git status --short --branch
```

The commit hashes must match and the worktrees must be clean. The test build
uses Iroh `0.29.0`, Rust/Cargo `1.98.1`, Node.js `20.20.2`, npm `10.8.2`,
`wasm-pack 0.15.0`, and the Rust target `wasm32-unknown-unknown`.

The Windows feasibility result supplied for this order verified those
versions. The detailed Windows installer checklist is not present in this
repository. On the already-verified Windows test laptop, verify rather than
reinstall:

```powershell
rustc --version
cargo --version
node --version
npm.cmd --version
wasm-pack --version
rustup target list --installed
```

Expected versions are Rust/Cargo `1.98.1`, Node `v20.20.2`, npm `10.8.2`,
wasm-pack `0.15.0`, and an installed `wasm32-unknown-unknown` target. Replacing
Node requires administrator approval; do not replace it without that approval.
If any prerequisite is missing or differs, pause and obtain the verified
Windows install checklist rather than guessing at an installer or changing
machine policy.

On Linux, verify the same versions with:

```sh
rustc --version
cargo --version
node --version
npm --version
wasm-pack --version
rustup target list --installed
```

If a Linux prerequisite is missing, install the exact Rust toolchain and
WASM target through rustup, install `wasm-pack 0.15.0` with
`cargo install wasm-pack --version 0.15.0 --locked`, and use the approved
Node.js `20.20.2` installation. Record any version deviation; do not silently
substitute a different version.

Both machines need Git, the Rust toolchain for their native platform, the
prerequisites above, and a supported desktop browser. The Windows test laptop
was verified at the baseline commit, but this new diagnostics commit has not
yet been built on Windows.

## Build and start each machine

From the repository root on each laptop, install the locked client packages:

Linux:

```sh
npm --prefix client ci
```

Windows PowerShell:

```powershell
npm.cmd --prefix client ci
```

PowerShell may block the `npm` script shim; use `npm.cmd` for npm commands.
The first native Rust build can be slow, especially on Windows. Let it finish;
do not interrupt it because it appears idle.

### Terminal 1: native gateway with diagnostics

Linux:

```sh
export HGS_CONNECTION_LOG_DIR="$HOME/hgs-we3-logs"
mkdir -p "$HGS_CONNECTION_LOG_DIR"
cargo run --locked -p hex_game_orchestrator --features connection-diagnostics
```

Windows PowerShell:

```powershell
$env:HGS_CONNECTION_LOG_DIR = Join-Path $env:USERPROFILE 'hgs-we3-logs'
New-Item -ItemType Directory -Force -Path $env:HGS_CONNECTION_LOG_DIR | Out-Null
cargo run --locked -p hex_game_orchestrator --features connection-diagnostics
```

The first build may take several minutes. When ready, the terminal prints
`P2P gateway ready at ws://127.0.0.1:8080`. The diagnostics feature is opt-in
and off in ordinary builds. It creates one local `hgs-we3-*.log` file per
native peer session when Host or Join starts; the terminal prints each new
filename. Keep this terminal open.

### Terminal 2: loopback-only browser client

Linux:

```sh
npm --prefix client run dev -- --host 127.0.0.1
```

Windows PowerShell:

```powershell
npm.cmd --prefix client run dev -- --host 127.0.0.1
```

This command-line `--host` setting overrides the Vite configuration that
otherwise listens on all interfaces. Use the printed
`http://127.0.0.1:1420/` URL. Do not use `--host 0.0.0.0`. The dev command
builds the WASM module before starting Vite.

## Optional diagnostics-off default recovery check

This check is optional. From the repository root, run it once without the
WE-3 `connection-diagnostics` feature:

```sh
cargo test --locked -p hex_game_orchestrator --test network_simulation network_simulation_auth_sync_disconnect_and_recovery -- --exact
```

Do not retry it if it fails. The test target may still print its pre-existing
WE-2 test-only route diagnostics; do not add `--features
connection-diagnostics`.

## Publisher and firewall prompts

An actionable Windows dialog stays open and presents buttons or other options.
For any such dialog, record its exact wording, the program named, and every
option shown, then follow the applicable instructions below. A transient
notification that disappears or only offers to open Settings is not that
dialog: record that it appeared and what it was about; do not change settings.

If Windows displays a publisher/SmartScreen notice, verify that the program is
the locally built `hex_game_orchestrator.exe` under this repository's
`target\debug` directory. Only for that expected local executable, use
**More info** then **Run anyway**. If the executable or path is unexpected,
cancel and report it.

If Windows displays **Windows Defender Firewall has blocked some features of
this app**, record the app name, publisher, profile checkboxes, and the button
clicked. Do not select additional network profiles or create a new allow rule;
choose **Cancel** to preserve the current firewall policy. In particular, do
not change the previously created Public-profile rules. If no prompt appears,
record that too. Never turn off the firewall.

Before every attempt, record the rule state on both laptops:

- Windows test laptop: open **Windows Defender Firewall with Advanced Security**
  (`wf.msc`) and inspect the enabled rule for `hex_game_orchestrator.exe` under
  **Inbound Rules**. A rule listed under Inbound Rules has Direction **Inbound**.
  Record Enabled, Action, Direction, and Profile. Do not edit the rule.
- Linux: use the existing firewall's read-only status view (for example,
  `sudo ufw status verbose` when UFW is installed). Record `inactive`,
  `active` plus the relevant existing rule, or `unknown` if no read-only view
  is available. Do not add or change a rule.

If a firewall prompt or policy blocks the planned run, do not work around it
with a tunnel, firewall change, or alternate executable.

## Host and join

On both laptops, open the local Vite URL, select **Live Network Mode
(WebSocket)**, and expand **Multiplayer Connection Portal**. Wait for the
local gateway connection status to show connected before continuing.

1. The host clicks **Host Match**. The portal shows **Your connection ticket**.
2. Copy the entire value, including the `@` and the match-ID suffix
   (`NodeID@browser_...`). Do not trim or retype the suffix.
3. Clear any old ticket from the shared text pad, then put only this current
   ticket there for the other laptop. The pad address must remain known only to
   the tester and must never be written in the repository.
4. The guest pastes it into **Host connection ticket** and clicks **Join Match**.
5. Success is shown as **Joined and synchronized with the host** on the guest
   and **Peer connected; action-token synchronization is active** on the host.
   The guest should be P2/Red and the host P1/Blue.
6. Start the join timer at the guest's Join click and stop it at the success
   message or a visible error. If the UI remains at **Connecting to Iroh
   host…** for 120 seconds, record `still pending at 120s`; this is an external
   observation cutoff, not a product timeout. Do not retry that same attempt.
7. Clear the pad after the attempt, including after a pending observation
   cutoff. Clear it again at the end of the session. Never put logs there.

### Two local clients on one laptop

For the required local telemetry smoke, start a second client server in another
terminal, after the first is ready:

Linux:

```sh
npm --prefix client run dev -- --host 127.0.0.1 --port 1421
```

Windows PowerShell:

```powershell
npm.cmd --prefix client run dev -- --host 127.0.0.1 --port 1421
```

Open `http://127.0.0.1:1420/` in one browser page and
`http://127.0.0.1:1421/` in another. Different ports make distinct browser
origins and independent session storage while both servers remain loopback-only.
Host from one page and join from the other using the complete ticket.

## What the diagnostic logs contain

Each line has a Unix-millisecond timestamp, local generated session ID, local
attempt number, side, stage, event/result, elapsed milliseconds, and the Iroh
version. Route observations are `direct`, `relay`, `both`, or `unknown`, with
counts of advertised direct candidates and a relay-candidate boolean. While a
connection is pending, the diagnostic build samples once per second; after
connection, it records a route change at the first sample that observes it.
These timestamps are observation times, not exact Iroh path-change times.
Stages include endpoint binding, QUIC Initial/handshake, peer identity,
authentication, match binding, snapshot transfer, and connection lifetime.

The logger does not write connection tickets, node IDs, match IDs, player tags,
IP addresses, authentication packets, snapshots, or key material. Invalid or
incomplete tickets rejected in the browser do not create a peer-connection log
because no peer connection is attempted. Keep each machine's log files with
that machine's recording rows; do not merge files or rename them to include a
ticket or node ID.

## Collecting results

Do not collect while a gateway is running. Stop every gateway with **Ctrl+C**
and wait for it to exit first; on Windows the running gateway holds its log
files open.

The default log folders are `$HOME/hgs-we3-logs` on Linux and
`%USERPROFILE%\hgs-we3-logs` on Windows. These defaults are intended to be
outside a repository checkout. The repository has no ignore rule for these
folders; if you override the log directory, keep it outside the checkout.

Make and fill in a separate copy of the recording sheet outside the repository,
then place the completed copy beside the logs. This external copy is expected;
editing it does not edit the repository. Do not fill in the checked-in template.

Linux, from the repository root:

```sh
set -e
cp docs/we3-test-package/recording-sheet.md "$HOME/recording-sheet-filled.md"
# Fill in $HOME/recording-sheet-filled.md in a text editor.
cp "$HOME/recording-sheet-filled.md" "$HOME/hgs-we3-logs/recording-sheet-filled.md"
find "$HOME/hgs-we3-logs" -maxdepth 1 -type f -printf '%f\n'
tar -czf "$HOME/hgs-we3-results.tar.gz" -C "$HOME" hgs-we3-logs
tar -tzf "$HOME/hgs-we3-results.tar.gz"
```

Windows PowerShell, from the repository root in a new terminal after the
gateway has stopped:

```powershell
$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($env:USERPROFILE)) { throw 'USERPROFILE is empty.' }
$env:HGS_CONNECTION_LOG_DIR = Join-Path $env:USERPROFILE 'hgs-we3-logs'
if ([string]::IsNullOrWhiteSpace($env:HGS_CONNECTION_LOG_DIR)) { throw 'HGS_CONNECTION_LOG_DIR is empty; refusing to archive.' }
$logDir = [System.IO.Path]::GetFullPath($env:HGS_CONNECTION_LOG_DIR)
$driveRoot = [System.IO.Path]::GetPathRoot($logDir)
if ($logDir.TrimEnd('\') -eq $driveRoot.TrimEnd('\')) { throw 'Refusing to use a drive root as the archive source.' }
if (-not (Test-Path -LiteralPath $logDir -PathType Container)) { throw "Log folder not found: $logDir" }
Copy-Item .\docs\we3-test-package\recording-sheet.md "$env:USERPROFILE\recording-sheet-filled.md"
# Fill in the copy in a text editor.
Copy-Item "$env:USERPROFILE\recording-sheet-filled.md" "$logDir\recording-sheet-filled.md" -ErrorAction Stop
if (-not (Test-Path -LiteralPath "$logDir\recording-sheet-filled.md" -PathType Leaf)) { throw 'Filled recording sheet is missing.' }
if (-not (Get-ChildItem -LiteralPath $logDir -Filter 'hgs-we3-*.log' -File)) { throw 'No diagnostic logs found.' }
$unexpected = @(Get-ChildItem -LiteralPath $logDir -File | Where-Object { $_.Name -ne 'recording-sheet-filled.md' -and $_.Name -notlike 'hgs-we3-*.log' })
if ($unexpected.Count -gt 0) { throw 'Unexpected files in log folder; do not archive.' }
Get-ChildItem -LiteralPath $logDir -File | Select-Object -ExpandProperty Name
$archivePath = Join-Path $env:USERPROFILE 'hgs-we3-results.zip'
Compress-Archive -Path "$logDir\*" -DestinationPath $archivePath -Force -ErrorAction Stop
if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) { throw 'Archive is missing; collection failed.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
try { $zip.Entries | Select-Object -ExpandProperty FullName } finally { $zip.Dispose() }
```

The Windows command lists archive entries read-only; record the displayed
names. A missing ZIP means collection failed. The Linux `tar -tzf` command
lists the Linux archive entries; record those names too. Each archive must
contain only diagnostic `.log` files and `recording-sheet-filled.md`. Check the
sheet itself: it must contain no ticket, node ID, or match ID. Do not include
screenshots, especially screenshots that show a ticket or ID. If any other
file appears, do not send that archive.

Approved ways to move results are a USB drive or the tester's own private cloud
folder. To move the Windows archive to Linux by USB, copy
`%USERPROFILE%\hgs-we3-results.zip` to the USB drive in File Explorer, safely
eject it, connect it to Linux, and copy the archive into the Linux user's home
folder. Never use the shared text pad for log files.

## Stop and verify cleanup

Press **Ctrl+C** in every gateway and Vite terminal. Wait for each command to
exit. Do not kill unrelated processes by name.

Linux:

```sh
ss -ltnp | grep -E ':(8080|1420|1421)\b'
```

Windows PowerShell:

```powershell
Get-NetTCPConnection -State Listen -LocalPort 8080,1420,1421 -ErrorAction SilentlyContinue
```

No output means the listed ports are closed. The single-client cross-network
setup uses ports 8080 and 1420; port 1421 is used only for the two-origin local
smoke.
