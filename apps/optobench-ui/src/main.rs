//! optobench's desktop interface (M8): SolidCraft's frame and design language around
//! optobench's workspaces (CAD, Layout, Simulate), running every action through the same
//! command registry as the command line.
//! `optobench-ui [design.optobench] [--data-directory <directory>]`.

#![forbid(unsafe_code)]

mod app;
mod camera;
mod document;
mod editors;
mod icons;
mod render;
mod screenshot;
mod theme;
mod viewport;
mod workspaces;

fn main() -> eframe::Result {
    let mut path = None;
    let mut data_directory = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--data-directory" {
            data_directory = args.next().map(std::path::PathBuf::from);
        } else {
            path = Some(std::path::PathBuf::from(arg));
        }
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        // Exit with the window rather than return to `main`: on macOS, returning leaves
        // AppKit's run loop to a Touch Bar observer of the destroyed view, which throws.
        run_and_return: false,
        viewport: egui::ViewportBuilder::default()
            .with_title("optobench")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "optobench",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            let renderer = cc.wgpu_render_state.as_ref().map(render::Renderer::new);
            Ok(Box::new(app::App::new(
                path,
                data_directory,
                renderer,
                cc.egui_ctx.clone(),
            )))
        }),
    )
}
