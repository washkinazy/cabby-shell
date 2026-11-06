# Solid Fluent-inspired appearance

Cabby's visual direction is solid Fluent 2: opaque neutral surfaces, modest corner rounding, compact headings, subtle borders and clear selected states. The bar keeps its configured edge, height and left/middle/right layout. There is no glass, backdrop blur, wallpaper tinting or translucent panel fill. This is a repository-owned Iced treatment inspired by [Fluent 2's solid material](https://fluent2.microsoft.design/material), not a Microsoft toolkit dependency.

The shared implementation in `crates/shell/src/ui/fluent.rs` styles the bar, audio/Bluetooth panels, tray menus, launcher, notifications, clipboard picker and screenshot controls. Native application/tray icons keep their original artwork. Existing lock rendering and security behavior are unchanged; the lock continues to use the existing shared color preferences.

## Surfaces and controls

- Main surfaces use the configured `background` exactly, with 8-pixel panel corners and a one-pixel neutral border.
- Controls and device groups use 4-pixel corners and small neutral surface steps. Ordinary actions are neutral; primary actions and active Caffeine use the accent with contrasting text.
- Workspace selection uses a short accent underline on a neutral fill. Launcher selection and selected audio devices use a narrow accent marker; device selection also retains its text label.
- Audio/Bluetooth devices have bounded, grouped controls. Audio keeps its heading outside scrolling; device actions wrap when constrained. Volume sliders have a narrow track and circular handle.
- Headings derive from body text size with a five-pixel increase and semibold weight. Supporting text uses a lower-emphasis foreground mix. Search fields use an accent focus border.
- Vulkan panel styling includes a restrained shadow. Software rendering omits the shadow because the pinned tiny-skia renderer darkens panel content when a clipped shadow is present. Opaque fill, border, geometry and interactions remain the same.

Surfaces step toward `foreground` from `background`, so both light and dark palettes work. Normal raised controls use a 5.5% mix, selected rows use 12%, selected device groups use 10%, borders use 20%, and secondary text uses 74%. These are fixed internal appearance rules, not new configuration settings. Primary text contrast uses Iced's existing palette derivation. Users remain responsible for choosing readable foreground/background pairs.

## Configuration and upgrades

Use the existing [`background`, `foreground`, `accent` and `font_size` settings](configuration.md#background). Their types, defaults and validation ranges are unchanged. Layout settings still own bar placement and density. Wallpaper rendering remains independent of UI surface colors. No configuration migration or dotfiles edit is required for this visual update, and no preferences are overwritten.

An updated build requires an ordinary shell restart to replace the running UI binaries. Subsequent valid color changes reload in place using the documented rules; launcher and lock preferences apply at their next opening. There is no theme picker or additional service.

## Validation scope

Style-contract tests check opaque fills and preservation of configured text/surface colors across light, dark and custom palettes, including interaction states. Nested pixel checks cover neutral workspace selection with an accent marker, exact opaque panel padding, selected Caffeine contrast and enabled screenshot controls. Existing private client/device fixtures continue to exercise behavior. Renderer results and environment blockers are recorded in [PLAN.md](../PLAN.md); passing software rendering never substitutes for the required RTX 4070 Vulkan check.
