mod ai;
mod ai_window;
mod blocks;
mod cmdline;
mod commands;
mod conversations;
mod drawing_host;
mod files;
mod launch;
mod mcp;
pub mod methods;
#[cfg(debug_assertions)]
mod mock;
mod pdf;
mod plot;
pub mod ribbon;
mod script;
pub mod secrets;
pub mod settings;
mod state;
mod tabs;
mod updater;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(launch::OpenRequests::default())
        .manage(updater::Pending::default())
        .manage(script::ScriptControl::default())
        .manage(cmdline::CommandLineState::default())
        // First of the plugins: a second Qiyas hands its files to this one and quits.
        .plugin(launch::single_instance())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(launch::os_opened())
        .setup(|app| {
            // A machine without a keychain still runs; saving a key then explains why it can't.
            if let Err(e) = secrets::use_native_store() {
                eprintln!("{e}");
            }
            let config_dir = app.path().app_config_dir()?;
            app.manage(state::AppState::new(config_dir));
            app.manage(files::Locks::default());
            files::install_panic_hook(app.handle().clone());
            files::start_watch(app.handle().clone());
            pdf::use_bundled_pdfium(app.handle());
            mcp::restart(app.handle());
            launch::request_launch_files(app.handle());
            updater::check_at_launch(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_drawing,
            commands::new_drawing,
            commands::recents,
            commands::clear_recents,
            commands::views,
            tabs::tabs,
            tabs::activate_tab,
            tabs::close_tab,
            commands::drawing_info,
            commands::set_sheet,
            commands::set_layer_hidden,
            commands::reload_xrefs,
            commands::set_xref_path,
            commands::batch_version,
            commands::render_batch,
            commands::atlas_pages,
            commands::sheet_batch,
            commands::sheet_batch_version,
            files::paper_views,
            files::clipboard_read_text,
            files::clipboard_read_image,
            files::autosave_drawings,
            files::recoverable_drawings,
            files::untitled_autosaves,
            files::recover_autosave,
            files::discard_autosave,
            files::recover_drawing,
            files::sheet_set_open,
            files::sheet_set_save,
            files::sheet_set_entry,
            files::sheet_set_from_tabs,
            commands::pick,
            commands::boundary_at,
            commands::pick_window,
            commands::pick_polygon,
            commands::snap,
            commands::get_drafting,
            commands::set_drafting,
            commands::entity_info,
            commands::table_cell_at,
            commands::run_command,
            cmdline::command_words,
            cmdline::cmdline_step,
            commands::undo,
            commands::redo,
            commands::takeoff,
            commands::totals,
            commands::save_project,
            commands::save_drawing,
            commands::save_drawing_as,
            commands::export_takeoff,
            commands::export_table,
            commands::import_boq,
            commands::boq,
            commands::compare,
            commands::conflicts,
            commands::export_conflicts,
            commands::get_settings,
            commands::set_settings,
            commands::aliases,
            commands::set_api_key,
            commands::has_api_key,
            commands::delete_api_key,
            commands::app_dir,
            commands::mcp_status,
            ribbon::get_default_ribbon,
            script::run_script,
            script::stop_script,
            updater::check_update,
            updater::install_update,
            updater::cancel_update,
            launch::take_open_requests,
            commands::ai_send,
            commands::ai_models,
            ai_window::ai_window_reply,
            commands::ai_cancel,
            commands::ai_clear,
            commands::ai_conversations,
            commands::ai_resume,
            commands::ai_delete_conversation,
            commands::ai_delete_conversations,
            methods::measurement_methods,
            commands::ai_status,
            commands::ai_log,
            commands::ai_set_language,
            commands::ai_undo,
            pdf::pdf_page_count,
            pdf::pdf_page_size,
            pdf::pdf_thumbnails,
            pdf::attach_pdf,
            pdf::is_dirty,
            pdf::calibrate,
            pdf::underlay_image,
            pdf::image_size,
            pdf::attach_image,
            pdf::image_display,
            blocks::blocks,
            blocks::block_thumbnail,
            blocks::drawing_blocks,
            blocks::drawing_layers,
            blocks::write_block,
            blocks::set_tool_palette,
            plot::paper_catalog,
            plot::plot_pdf,
            plot::plot_pdf_to,
            plot::plot_and_print,
            plot::print_pdf,
            plot::list_printers,
            plot::default_printer,
            plot::load_plot_style,
            plot::save_plot_style,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Qiyas")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<state::AppState>() {
                    commands::ai_run(&state).save_logged();
                }
                files::release_locks(app);
            }
        });
}
