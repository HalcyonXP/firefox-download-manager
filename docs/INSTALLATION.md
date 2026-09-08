# Windows 11 / Firefox Developer Edition installation

Use a qualified release from **https://github.com/HalcyonXP/firefox-download-manager/releases**. CI candidates and development builds are not release approval. The release notes identify the tested ZIP checksum and limitations. First-party licensing is not specified; third-party notices are included separately.

## Prerequisites

- Windows 11, x64 package; Firefox Developer Edition **156 or later**. Only the explicitly recorded browser/OS configurations are qualified.
- No administrator elevation, Rust, Node.js or Python is required to run the packaged setup/helper. The release recipe statically links reviewed LLVM/MinGW support and uses Windows’ built-in UCRT; inspect `BUILD-INFO.json` for imported Windows DLLs and build provenance.
- Close Firefox and running native helpers before install, upgrade, cleanup, uninstall, repair or recovery. Setup refuses them; it does not terminate them. It does not change execution policy, firewall, routing, VPN, certificate trust, signing preferences, or any browser profile.
- The helper and XPI are **unsigned**. Obtain them from the canonical repository, compare the ZIP's SHA-256 against the release's `PACKAGE-SHA256SUMS.txt`, and review any Windows security warning yourself. Checksums detect inconsistent bytes, not a compromised publisher or malicious package with matching edited metadata. Do not disable system protections to force an install.

## Install

1. Download the versioned Windows-x64 ZIP and its separate `PACKAGE-SHA256SUMS.txt`. In PowerShell, `Get-FileHash .\firefox-download-manager-0.1.0-windows-x64.zip -Algorithm SHA256` must match the release's value.
2. Extract the ZIP into an ordinary local directory. Keep the extracted package for later uninstall/recovery. Do not run from inside the ZIP or a network/reparse directory.
3. Open a terminal in that directory and run:

   ```powershell
   .\download-manager-setup.exe verify
   .\download-manager-setup.exe install
   ```

   `verify` only reads/checks payloads. `install` copies a verified generation and tests the helper with fresh temporary state before current-user registration.
4. Open Firefox Developer Edition's `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on**, and select the XPI from the generation directory printed by setup, beneath:

   `%LOCALAPPDATA%\HalcyonXP\FirefoxDownloadManager\host`

   The filename is `firefox-download-manager.xpi`. Choose the **installed generation**, not a differently built extension. This temporary-add-on workflow does not require changing signing preferences. **Reload the XPI after every Firefox restart.** Setup never installs it into your profile automatically. Permanent unsigned installation/signing is not provided or implied.
5. Use **Download with Manager** on a direct HTTP(S) link, or the extension toolbar/manager page. Session handoff is unchecked by default and optional. Blank SHA-256 means structural validation only; a supplied digest is checked before publication.

Custom root: append `--root "$env:LOCALAPPDATA\My Manager Host"` to every mutating command. It must be fully drive-qualified, ordinary/non-reparse, beneath local application data, disjoint from task state, and at most **160 UTF-16 units**. Names with spaces are supported. UNC/device paths, traversal, reserved/ambiguous Windows components and unknown collisions are refused. A second root cannot replace another root's registration.

## Upgrade and remove

- **Upgrade:** close Firefox/helpers; extract and verify the new qualified package; run its `install` with the same root. A new immutable generation becomes current only after verification. Reload its XPI explicitly. State bytes are preserved; the helper handles documented compatible state migration on its next normal start. No automatic update or background download occurs.
- **Cleanup:** `download-manager-setup.exe cleanup` removes verified retired generation files only. It preserves the current generation and unknown files. At most four generations are recorded; run cleanup before a fifth install. Existing unreceipted development installations are **not automatically adopted**: they require owner inspection/removal, never the old unsafe convenience scripts as a fallback.
- **Uninstall:** `download-manager-setup.exe uninstall` removes the matching current-user native registration and verified generated program files. It leaves task state, partials, completed downloads, unknown files and parent directories intact. It does not edit Firefox; remove the temporary add-on in Firefox if still listed. Empty installation directories and the zero-byte cooperative `setup.lock` may remain.

Task/settings/diagnostic state is separate:
`%LOCALAPPDATA%\HalcyonXP\FirefoxDownloadManager\state`.
Exact signed URLs and server validators may be sensitive recovery data. Do not upload this folder or raw browser profiles/logs for troubleshooting.

## Failure and recovery

- No generic force/overwrite switch exists. Foreign registration, unknown/changed files, junctions and unsupported state are preserved and rejected.
- If `transaction.json` remains, close Firefox/helpers and run **`download-manager-setup.exe recover`** with the same root. Interrupted install recovery restores the proven previous generation (or absence), not unverified new authority. Interrupted uninstall/cleanup finishes removal of proven generated files; it does not restore partially removed programs.
- With no journal, `repair` can rebind the verified current generation if registration is absent or still points to another generation recorded in the same receipt. It probes and rechecks the current helper, never adopts a foreign registration or changes task state. This handles explicitly inspected stale-registration cases without blessing unknown bytes.
- Failed launch/ordinary injected failure is rolled back. An interruption before journal creation can leave an unrecorded directory. Torn copies, malformed journals, changed registration or inconsistent bytes may require explicit owner inspection; setup will not guess that they are safe to delete. Keep the package and remaining receipt/journal, preserve state/downloads, and consult the repository's security/packaging documentation. Never hand-edit a journal to bless arbitrary paths or use recursive deletion of the application-data base.
- Journal recovery covers tested process/failure boundaries. Windows registry hive persistence and filesystem persistence are not one power-loss transaction. Hardware faults, power loss and malicious same-account namespace/memory mutation are not claimed solved; inconsistent records fail explicitly rather than authorizing a guessed overwrite.
- `probe` runs the verified helper in fresh temporary state without registration or browser use; it does not test the Firefox UI or your existing tasks. `--help` lists commands. Errors intentionally omit raw untrusted paths/state/credentials.

The full architecture, protocol/state compatibility, security review and qualification evidence are in the canonical repository's `docs` directory. No torrent/media extraction, blanket download interception, telemetry, cloud sync, VPN integration or remote updater is included.
