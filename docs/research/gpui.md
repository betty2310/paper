# Research: GPUI for a standalone cross platform app

Verified 2026-10-27 against zed main (`fdcb198`), longbridge/gpui-kit main (`c8917c3`), crates.io, and the linked pages.

## Distribution

The official `gpui` crate on crates.io is 0.2.2, published 2025-10-22, Apache-2.0, and has had no release since. It still uses the blade renderer on Linux, while zed main has moved to wgpu, so 0.2.2 does not reflect the current API. In zed main only `crates/gpui` is publishable, and the satellite crates the current API needs (`gpui_platform`, `gpui_macos`, `gpui_windows`, `gpui_linux`, `gpui_wgpu`, `gpui_tokio`, `gpui_macros`) are `publish = false`. There is no standalone `zed-industries/gpui` repository. The README says "pre-1.0. There will often be breaking changes." The entry point for standalone apps is `gpui_platform::application().run(|cx: &mut App| ...)`.

Third party snapshots on crates.io:

| Crate | Version | Note |
|---|---|---|
| `gpui-pre` (longbridge) | 0.3.10, 2026-10-26 | weekly snapshot of zed main; companions `gpui-pre-platform`, `gpui-pre-macros`, `gpui-pre-web`, `gpui-pre-reqwest-client`. No `gpui-pre-tokio`. |
| `gpui-fast` (longbridge) | 0.1.12 | retained mode performance fork, "remains experimental" |
| `gpui-unofficial` | 1.25.3 | mirrors Zed release tags |
| `gpui-ce` | 0.2.2 | Parley text, API diverging |

Mixing GPUI from two sources in one build fails with duplicate type errors ("The GPUI Distribution Problem", gpui-archipelago.github.io/news/gpui-distribution). Upstream policy (gpui-component discussion #1856): "Code that isn't directly usable in Zed isn't being merged in upstream GPUI". Custom shader PR zed#42905 and tray PR zed#44047 were rejected.

## Platform support

macOS uses Metal, native NSMenu menus, native dialogs, IME. It is the most mature target. Windows uses Direct3D 11, DirectWrite, DirectComposition; `set_menus` only stores menus, so there is no native menu bar; native file dialogs; IME handled; AccessKit via `accesskit_windows`. Linux renders through wgpu (Vulkan), Wayland and X11 backends, xdg-desktop-portal file dialogs, `accesskit_unix`, headless mode. Vulkan is required and `NoSupportedDeviceFound` is the common failure (zed docs/src/linux.md). AccessKit is wired on all platforms but a November 2025 HN report says Zed is "opaque to screen readers".

## gpui-kit (formerly gpui-component)

Crates at 0.8.1 (2026-10-26, Apache-2.0): `gpui-kit` (the one dependency), `gpui-component`, `gpui-base`, `gpui-kit-assets`, `gpui-webview` (wry), `gpui-shell`. `gpui-kit 0.8.1` pins `gpui-pre =0.3.10`. It tracks longbridge's weekly snapshots, not zed git main, and all pins are exact.

Widgets: accordion, alert, button, chart, plot, checkbox, color picker, combobox, command palette, dialog, dock (resizable panels, draggable tabs, nested splits, serializable), form, input (single and multi line, code editor), list, searchable list, menu, native menu, pagination, popover, progress, resizable, scroll with scrollbar, select, sheet, sidebar, slider, switch, tabs, table (virtual scrolling, resizable sortable columns), text, title bar, toolbar, tooltip, tree, virtual list (variable sizes), calendar, date picker, number input, focus trap, undo history, text selection, Markdown and HTML rendering, webview. Headless UI testing is supported. Agent skills: `npx skills add longbridge/gpui-kit`.

Other GPUI crates of interest: `gpui-pdf` 0.6.1 (hayro based viewer by packetthrower, useful as a reference for the page list), `gpui-router`, `gpui-devtools`, `gpui-updater`.

## Images and custom paint

`img(ImageSource)` with `ImageSource::Render(Arc<RenderImage>)`. `RenderImage::new(frames)` expects BGRA (assets.rs: "cached and processed image, in BGRA format"); the loader swizzles RGBA with `pixel.swap(0, 2)`. Low level: `Window::paint_image(bounds, image_bounds, corner_radii, Arc<RenderImage>, frame_index, grayscale)` uploads into the sprite atlas, `Window::drop_image(Arc<RenderImage>)` evicts, plus `paint_quad`, `paint_path`, `paint_layer`, `paint_svg`. The `canvas(prepaint, paint)` element gives a free paint area. Images are keyed by `Arc` identity, so reuse the same `Arc` across frames. Texture limits: 16384 on Metal and DirectX, `max_texture_dimension_2d` on wgpu (8192 to 16384). No custom GPU shaders. Examples: `crates/gpui/examples/{image_gallery,image_loading,painting}.rs`.

## Lists

`uniform_list` for equal height rows (library table, or use gpui-component `Table`). `list(ListState, render_item)` for differing heights backed by a sum tree, with `ListState::new(count, alignment, overdraw)`, `splice`, `reset`, `scroll_to`, `scroll_to_reveal_item`, `bounds_for_item`, `viewport_bounds`. Suitable for the PDF page stack.

## Async

Executors are Zed's own `scheduler` crate, not tokio and not smol. Main thread: `cx.spawn(async move |cx: &mut AsyncApp| ...)` returns `Task<T>`, dropped tasks are cancelled, `.detach()` to keep. Background: `cx.background_spawn(fut)` or `cx.background_executor().spawn(...)`, real threads, so blocking calls are fine. Pattern: `let bitmap = cx.background_spawn(async move { render() }).await; entity.update(cx, |s, cx| { s.pages.insert(ix, img); cx.notify(); })`. `crates/gpui_tokio` exists upstream (about 100 lines, creates a 2 thread runtime as a Global) but is unpublished; copy it if tokio is needed.

## State model

`cx.new(|cx| T)` gives `Entity<T>`; `entity.read(cx)`, `entity.update(cx, |t, cx: &mut Context<T>| ...)`; `WeakEntity` for back references. Views implement `Render::render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement` and call `cx.notify()`. Reactivity: `cx.observe(&entity, ...)` and `cx.subscribe(&entity, ...)` return `Subscription` (store it or `.detach()`), events via `EventEmitter<E>` and `cx.emit`. Globals: `cx.set_global`, `cx.global::<G>()`, `update_global`, `observe_global`. Recommended split: a few long lived app level entities created in `application().run` and passed by handle; each view is its own entity holding handles and subscriptions.

## Real apps and pain points

Longbridge Pro (commercial, three platforms), Cap (screen recorder rebuilt from Tauri; CPU 76.6% to 17.5%, memory 3724 to 440 MiB, blog 2026-10-21), about 60 apps listed at gpui-kit.com/apps, Zorite, Loungy. Common complaints: only API reference docs, clean builds of 10 minutes or more, ecosystem fragmentation across snapshot crates, breaking changes.

## Build and packaging

Linux build deps from zed `script/linux`: gcc, g++, libasound2-dev, libfontconfig-dev, libglib2.0-dev, libssl-dev, libvulkan1, libwayland-dev, libx11-xcb-dev, libxkbcommon-x11-dev, libzstd-dev, cmake, clang, lld. Runtime: Vulkan loader and driver, xdg-desktop-portal. Hello world release binary about 4 MB; expect 15 to 40 MB with PDFium. macOS: `codesign --options runtime --timestamp` and `notarytool`. Windows: Inno Setup, DPI awareness manifest (`windows-manifest` feature or provide your own; gpui-component uses `windows-manifest-provided-by-host`).

## Risks

1. No current official crates.io release; the API is reachable only through snapshots or git.
2. Weekly breaking changes; every `gpui-pre` bump may need code changes.
3. Fragmentation and duplicate crate errors if two sources are mixed.
4. Single vendor dependency on Longbridge for both snapshot and widgets.
5. Linux needs Vulkan; no GL fallback by default.
6. No native menu bar on Windows and Linux.
7. Screen reader support incomplete.
8. Atlas based image rendering; large pages must be tiled and cached by `Arc` identity.
9. Thin documentation.
10. Long compile times.

## Recommendation

```toml
[dependencies]
gpui-kit = "=0.8.1"   # re-exports gpui (gpui-pre 0.3.10), gpui-base, gpui-component
```

Do not use official `gpui` 0.2.2. Skip `gpui-fast` until it leaves experimental status (it is a gpui-kit feature flag). Keep a `[patch.crates-io]` escape hatch to a zed git rev only for a blocking upstream bug.
