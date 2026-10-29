# Wallpaper

Cabby renders static wallpaper using its existing Rust/Iced Wayland stack. No swaybg, GTK interface, or extra system package is required. A separate `cabby-wallpaper.service` owns background surfaces on all outputs, with the same renderer selection as the bar. It does not request keyboard focus, receive pointer input, or reserve workspace area.

By default, it draws the solid `wallpaper_color` (`#181b22`). To use an image:

```toml
wallpaper_image = "wallpapers/desktop.png"
wallpaper_color = "#181b22"
wallpaper_mode = "fill"
```

Place that image under `$HOME/.config/cabby-shell/wallpapers/` when using the default config. Relative paths resolve against the selected config file's directory, including with `--config`. The runtime snapshot contains the absolute path, so systemd's working directory cannot change its meaning. No files are copied, downloaded, or selected automatically.

PNG and JPEG are supported. The image is placed independently on every output; all outputs currently share the same image, mode, and color. `fill` crops while preserving aspect ratio, `fit` shows the whole image with color borders, `stretch` fills without preserving aspect ratio, and `center` uses native image dimensions as logical pixels. Transparent pixels reveal the configured color. Geometry changes use Iced/Wayland output sizing and scale handling. Tiling, slideshows, per-output overrides, and EXIF auto-rotation are outside this increment.

Run `cabby validate` before `cabby reload`. The coordinator fully decodes a candidate image within resource limits before accepting a reload; missing/corrupt/oversized images leave the previous active settings unchanged. Reload also detects changed image bytes at the same path. A changed configuration or image republishes the snapshot and queues bar and wallpaper restarts. Unchanged reloads queue neither. The current policy restarts both components even if only one needs the new setting.

Wallpaper pixels stay in memory while running. There is no file watcher or recurring rendering timer in Cabby's wallpaper component. Replace files atomically before reload; modification/deletion after validation can still make a restarted component fail. An old image is not persisted as a recovery copy. Diagnostics report errors without logging image contents. Resource limits, defaults, and examples are in the [configuration reference](configuration.md).

The target starts wallpaper automatically; the coordinator upholds it after coordinator recovery. It has bounded on-failure restarts and cgroup cleanup, stops with the shell or compositor, and stays independent of ordinary bar crashes. Do not run a competing background owner on the same outputs when installing Cabby. Nothing in this branch installs or enables services on the live session.

For supervised development, `cabby wallpaper --config PATH` runs the wallpaper executable in the foreground. The packaged paths expect `cabby-wallpaper` alongside `cabby`. Managed status and recovery use:

```sh
systemctl --user status cabby-wallpaper.service
journalctl --user -u cabby-wallpaper.service
systemctl --user reset-failed cabby-wallpaper.service
```

Tests cover relative-path resolution, same-path content changes, invalid image rollback, failed snapshot publication, PNG alpha, JPEG decoding, resource limits, and mode geometry. The nested harness compares captured pixels for solid color and all four modes, checks scaled center placement, and requires hardware Vulkan selection separately from software success. Systemd fixtures verify reload/recovery/cleanup with a process stand-in. Physical multiple outputs and hotplug remain unverified; the synthetic geometry and existing output-association tests cover application logic only. Idle CPU/memory budgets remain Phase 7 work.
