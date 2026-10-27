# RazerLite

A small Synapse replacement for a **Razer Huntsman V2 Analog** keyboard and a **Viper V3 Pro** mouse.
Rust + egui, one 4.9 MB exe. Runs in the tray like Synapse: **12 MB RAM, 0% CPU** in the background
(Synapse: ~1.3 GB across 14 processes). All settings are saved to the devices' onboard memory.

## How it runs

- **Tray icon**: 1 click = quick popover, 2 clicks = full window, right-click = Open / Start with Windows / Quit.
  Tooltip shows the mouse battery; the icon gets an amber dot when it's low and goes grey when Synapse is blocking.
- **Each window is its own short-lived process** (`razerlite --window full|popover`). Closing it frees the GPU
  driver's memory (~90 MB), so the tray process stays tiny. The close button hides to tray (setting).
- **Start with Windows**: HKCU Run key → `razerlite.exe --tray`. Toggle in Settings or the tray menu.
- **Low-battery notification**: Windows toast at the mouse's warning level (battery is read every 5 minutes).
- Single instance: launching again opens the window of the running copy.

## Device images

The window and popover use Razer's own product renders (Huntsman V2 Analog US layout, Viper V3 Pro black),
copied from the local Synapse cache (`%LOCALAPPDATA%\Razer\RazerAppEngine\User Data\Default\Service Worker\CacheStorage`).
They're Razer's images, so they're **not in git**: put `huntsman_v2_analog.png` and `viper_v3_pro.png` in
`assets\devices\` (next to the exe or in the repo) or `%APPDATA%\RazerLite\devices\`. Without them the app
draws its own line art. The keyboard photo is split into a grey base and a light layer tinted with the current
lighting color (Spectrum/Wave show the original rainbow render). Key hit areas are mapped onto the photo
(`ui/images.rs`: `KB_ORIGIN`, `KB_UNIT`).

## Features

**Tray popover** (live art): mouse drawing fills with battery level (amber/red when low), DPI stage + polling
chips; keyboard outline glows in its current lighting color, brightness slider, effect chips, color swatches.

**Keyboard (Huntsman V2 Analog, PID 0x0266)**: Customize / Actuation / Lighting tabs on a drawn keyboard.
Drag to select keys (Alt+drag removes, click toggles, Ctrl+click adds).
- Actuation: press point (1.5–3.6 mm) and release point on one slider with two dots, per key; presets
  (Typing, Gaming = WASD shallower, Light touch, Deep). Normal + Hypershift layers are written together.
- Customize: remap keys to another key (+ Ctrl/Shift/Alt/Win), a mouse button, or disable; Standard/Hypershift.
- Debounce (press / release ms), polling rate (1000 / 500 / 125 Hz)
- Lighting: static, spectrum, wave, breathing, reactive, starlight, off; color, brightness, speed

**Mouse (Viper V3 Pro, PID 0x00C1 dongle / 0x00C0 wired)**: Performance / Power tabs.
- DPI stages (1–5), active stage, polling rate 125–8000 Hz
- Battery and charging, sleep timer, low-battery warning level

**Macros**: record keys with delays (recorded / fixed 20 ms / none), edit, stored in
`%APPDATA%\RazerLite\config.json`. Not yet written to the keyboard (needs Synapse's macro-memory commands
decoded and tested), so they can't be assigned to keys yet.

**Not yet**: mouse button remapping (different command, not decoded), onboard macros.

**Not available**: Rapid Trigger and Snap Tap. On the V2 Analog these run inside Synapse's software
(it reads the live analog stream and injects keys through its filter driver). The keyboard firmware
rejects the rapid-trigger commands that newer analog keyboards use.

Double typing fix without Rapid Trigger: give keys a release point well above the press point
(e.g. press 2.0 mm, release distance 0.4–0.5 mm), and/or set release debounce to 3–5 ms.

## Synapse must be fully uninstalled first

Do **not** just close Synapse. Razer's input filter driver (`RZVIRTUAL` / `RZCONTROL` devices)
sends every keystroke through Synapse. With the driver installed and Synapse closed, **the keyboard stops typing**.
While Synapse is running, it also sends the keyboard lighting frames nonstop, so keyboard settings can't be read or written (the mouse works either way).

To switch over:
1. In Synapse, set what you want once (or skip; RazerLite can set it after).
2. Settings → Apps → uninstall **Razer Synapse**, **Razer Chroma**, **Razer App Engine** (and Razer Central if listed). Reboot.
3. Device Manager → View → Devices by connection. If any **Razer Control Device** or `RZVIRTUAL` entries remain,
   uninstall each with "Attempt to remove the driver" ticked. Unplug and replug the keyboard and dongle.
4. Run `target\release\razerlite.exe`.

If the keyboard ever stops typing while Synapse's driver is still installed, start Synapse again:
`"C:\Program Files\Razer\RazerAppEngine\RazerAppEngine.exe" --url-params=apps=synapse,chroma-app --launch-force-hidden=synapse,chroma-app --autoStart=1`

## Build

```
cargo build --release
```

## Dev: UI without devices, and driving the app

- `razerlite --demo` uses fake devices and writes nothing (it does share the real config.json).
- `razerlite --demo --dev-mcp --tray` also starts an HTTP MCP server on `127.0.0.1:47851/mcp`
  (env `RAZERLITE_MCP_PORT`). Tools: `open {mode}`, `close`, `ui_state`, `navigate {tab, sub}`,
  `screenshot` (PNG, also saved to `%TEMP%\razerlite-shot.png`), `click`, `hover`, `drag`, `key`, `type`,
  `scroll`, plus all device tools below. The tray process answers `open` and forwards window tools to the
  window process on port+1. With the dev server on, the popover doesn't close on focus loss.
- `RAZERLITE_LOG=1` logs tray events and panics to `%TEMP%\razerlite.log`.
- Fast builds: `cargo build --profile fast` (~10 s incremental) → `target\fast\razerlite.exe`.

## Device MCP server

`razerlite.exe --mcp` runs a stdio MCP server (no window). Register it with Claude Code:

```
claude mcp add razerlite -- X:\Dev\razerlite\target\release\razerlite.exe --mcp
```

Tools: `list_interfaces`, `state`, `kb_keys`, `kb_set_actuation`, `kb_set_debounce`, `kb_set_polling`,
`kb_set_brightness`, `kb_set_effect`, `mouse_set_dpi`, `mouse_set_polling`, `mouse_set_sleep`,
`mouse_set_low_battery`, and `raw` (send any 90-byte report; writes need `allow_write`,
calibration class 0xFE and bootloader/reset IDs are blocked).

## Protocol notes

Razer 90-byte feature report: `[status, txn, remaining(2), proto, size, class, id, args[80], xor(2..88), 0]`.
Keyboard control interface: MI_03 (usage page 0x0C). Mouse: MI_00 (usage page 0x01, usage 0x02).

| What | Get | Set | Payload |
|---|---|---|---|
| Key assignment (incl. actuation) | `02:92` | `02:12` | `[profile, keyId, mode, press, release, fnType, fnLen, fnData…]`, press/release byte = mm × 255/3.6, `ff` = default |
| Debounce | `02:82` | `02:02` | `[pressDebounce, pressMask, releaseDebounce, releaseMask]` ms |
| Profiles | `05:81` list, `05:84` active | | |
| KB polling | `00:85` | `00:05` | 1=1000, 2=500, 8=125 Hz |
| Lighting | `0f:82` | `0f:02` | `[1, 0, effect, …]` (OpenRazer extended matrix) |
| Brightness | `0f:84` | `0f:04` | `[1, 0, value]` |
| Mouse DPI | `04:85` | `04:05` | `[1, xHi, xLo, yHi, yLo, 0, 0]` |
| DPI stages | `04:86` | `04:06` | `[1, active, count, {id, x(2), y(2), 0, 0}…]` |
| Mouse polling | `00:c0` | `00:40` | `[0, code]` 1=8k 2=4k 4=2k 8=1k 0x10=500 0x20=250 0x40=125 |
| Battery / charging | `07:80` / `07:84` | | `args[1]` 0–255 |
| Sleep | `07:83` | `07:03` | seconds, u16 BE |
| Low battery | `07:81` | `07:01` | 0–255 |

Key IDs follow the Razer matrix (W=18, A=31, S=32, D=33); full table in `src/keymap.rs`.
All of this was read out of Synapse 4's own device code (cached JS in `%LOCALAPPDATA%\Razer\RazerAppEngine`).
