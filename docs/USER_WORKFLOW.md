# Target workflow — next release, not available in v0.1.0

1. Run **setup.exe**. It installs and starts the companion app.
2. Check that **Download Manager appears in the system tray**.
3. **Install the XPI in Firefox once**, accepting its normal installation prompts.
4. **Restart Firefox**.
5. **Click a download link normally**. Manager adds it and starts downloading.

That is the required experience. Users should not need developer mode, terminal commands, repeated add-on loading or a right-click workaround.

**Current limitation:** v0.1.0 does not implement this workflow. It has manual capture, no tray companion and a temporary-XPI installation path. There is no interception setting you missed. The replacement is tracked in [M5](https://github.com/HalcyonXP/firefox-download-manager/milestone/6).

Unsupported downloads must stay in Firefox with a clear explanation, not disappear or download twice. The companion must have a clear status and Quit action. Persistent installation must work without weakening Firefox or Windows protections.
