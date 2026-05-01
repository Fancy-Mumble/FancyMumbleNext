//! Tauri application entry point.
//!
//! All `#[tauri::command]` handlers live in the [`commands`] submodule;
//! this file is responsible for wiring the application together
//! (logging, plugins, state, command registration and the event loop).
//!
// All public command functions receive `tauri::State` by value, which is
// required by the `#[tauri::command]` macro - suppress the lint crate-wide.
#![allow(clippy::needless_pass_by_value, reason = "tauri::command requires State<T> to be taken by value")]
// This is an application crate; pub items inside private modules are
// intentional (proc-macro visibility, Tauri command system, internal APIs).
#![allow(unreachable_pub, reason = "application crate: pub items in private modules are intentional for Tauri command system")]

mod audio;
pub(crate) mod commands;
pub mod platform;
mod state;
#[cfg(not(target_os = "android"))]
mod updater;

use state::AppState;
use std::sync::OnceLock;
use tauri::Manager;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Global handle for reloading the tracing filter at runtime.
pub(crate) static LOG_RELOAD_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

/// Entry point for the Tauri application.
///
/// Initialises the TLS crypto provider, sets up logging, registers all
/// Tauri commands, and starts the application event loop.
#[allow(clippy::expect_used, reason = "Tauri builder failure during startup is unrecoverable")]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    if platform::try_single_instance() {
        return;
    }

    platform::init();
    init_logging();
    platform::check_dependencies();

    let builder = create_base_builder();
    let builder = register_commands(builder);

    builder
        .manage(AppState::new())
        .setup(move |app| {
            init_app_state(app);
            platform::setup(app.handle().clone());
            #[cfg(not(target_os = "android"))]
            if let Err(e) = platform::desktop::tray::setup_tray(app) {
                tracing::warn!("Failed to create system tray icon: {e}");
            }
            #[cfg(not(target_os = "android"))]
            {
                // Force-hide the main window: the window-state plugin may
                // have just shown it after restoring saved geometry.
                if let Some(win) = app.get_webview_window(updater::MAIN_WINDOW_LABEL) {
                    let _ = win.hide();
                }
                updater::init(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                if let Some(state) = window.try_state::<AppState>() {
                    if let Ok(mut s) = state.inner.snapshot().lock() {
                        s.prefs.app_focused = *focused;
                    }
                }
            }
            #[cfg(not(target_os = "android"))]
            if matches!(event, tauri::WindowEvent::Destroyed)
                && window.label() == updater::UPDATER_WINDOW_LABEL
            {
                updater::show_main_window(&window.app_handle().clone());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.shutdown_offload_store();
                }
                platform::teardown();
            }
        });
}

fn init_logging() {
    let default_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into());
    let filter = EnvFilter::try_new(&default_filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload_handle) = reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(tracing_subscriber::fmt::layer())
        .init();
    let _ = LOG_RELOAD_HANDLE.set(reload_handle);
}

fn init_app_state(app: &mut tauri::App) {
    let state = app.state::<AppState>();
    state.set_app_handle(app.handle().clone());
    if let Err(e) = state.init_offload_store() {
        tracing::warn!("Failed to initialise offload store: {e}");
    }
}

fn create_base_builder() -> tauri::Builder<tauri::Wry> {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init());

    #[cfg(target_os = "android")]
    let builder = builder.plugin(
        tauri::plugin::Builder::<tauri::Wry, ()>::new("connection-service")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.fancymumble.app",
                    "ConnectionServicePlugin",
                )?;
                let cs_handle = platform::android::connection_service::ConnectionServiceHandle(handle);
                platform::android::connection_service::register_disconnect_listener(&cs_handle, app.clone());
                platform::android::connection_service::register_navigate_listener(&cs_handle, app.clone());
                let _ = app.manage(cs_handle);
                Ok(())
            })
            .build(),
    );

    #[cfg(target_os = "android")]
    let builder = builder.plugin(
        tauri::plugin::Builder::<tauri::Wry, ()>::new("fcm-service")
            .setup(|app, api| {
                let handle = api.register_android_plugin(
                    "com.fancymumble.app",
                    "FcmPlugin",
                )?;
                let fcm_handle = platform::android::fcm_service::FcmPluginHandle(handle);
                let _ = app.manage(fcm_handle);
                Ok(())
            })
            .build(),
    );

    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(
        tauri_plugin_window_state::Builder::new()
            // Restore size/position/maximised state, but NEVER restore
            // visibility. The updater module decides whether the main
            // window should appear on launch.
            .with_state_flags(
                tauri_plugin_window_state::StateFlags::all()
                    & !tauri_plugin_window_state::StateFlags::VISIBLE,
            )
            // Don't track the updater window - it has a fixed size set
            // in window.rs that must not be overridden by stale state.
            .with_denylist(&[updater::UPDATER_WINDOW_LABEL])
            .build(),
    );

    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    #[cfg(not(target_os = "android"))]
    let builder = updater::register_plugins(builder);

    builder
}

macro_rules! all_command_handlers {
    () => {
        tauri::generate_handler![
            commands::connection::connect,
            commands::certificates::generate_certificate,
            commands::certificates::list_certificates,
            commands::certificates::delete_certificate,
            commands::certificates::export_certificate,
            commands::certificates::import_certificate,
            commands::connection::disconnect,
            commands::connection::get_status,
            commands::servers::list_servers,
            commands::servers::get_active_server,
            commands::servers::set_active_server,
            commands::servers::disconnect_server,
            commands::channels::get_channels,
            commands::channels::get_users,
            commands::channels::get_user_texture,
            commands::channels::get_channel_description,
            commands::messaging::get_messages,
            commands::messaging::send_message,
            commands::messaging::edit_message,
            commands::channels::select_channel,
            commands::channels::join_channel,
            commands::channels::get_current_channel,
            commands::channels::toggle_listen,
            commands::channels::get_listened_channels,
            commands::channels::get_push_subscribed_channels,
            commands::channels::get_unread_counts,
            commands::channels::mark_channel_read,
            commands::server::get_server_config,
            commands::server::get_server_info,
            commands::server::get_welcome_text,
            commands::channels::update_channel,
            commands::channels::create_channel,
            commands::channels::delete_channel,
            commands::server::ping_server,
            commands::public_servers::fetch_public_servers,
            commands::public_servers::fetch_file_server_capabilities,
            commands::audio::get_audio_devices,
            commands::audio::get_output_devices,
            commands::audio::get_audio_settings,
            commands::audio::get_denoiser_param_specs,
            commands::audio::get_available_denoiser_algorithms,
            commands::audio::set_audio_settings,
            commands::audio::set_audio_backend,
            commands::audio::get_audio_backend,
            commands::audio::get_voice_state,
            commands::audio::enable_voice,
            commands::audio::disable_voice,
            commands::audio::toggle_mute,
            commands::audio::toggle_deafen,
            commands::audio::set_user_volume,
            commands::audio::start_mic_test,
            commands::audio::stop_mic_test,
            commands::audio::calibrate_voice_threshold,
            commands::audio::start_latency_test,
            commands::audio::stop_latency_test,
            commands::audio::start_recording,
            commands::audio::stop_recording,
            commands::audio::get_recording_state,
            commands::profile::set_user_comment,
            commands::profile::set_user_texture,
            commands::profile::get_own_session,
            commands::profile::send_plugin_data,
            commands::files::upload_file,
            commands::files::cancel_upload,
            commands::files::download_file,
            commands::files::add_custom_emote,
            commands::files::remove_custom_emote,
            commands::realtime::send_push_update,
            commands::realtime::send_subscribe_push,
            commands::messaging::send_read_receipt,
            commands::messaging::query_read_receipts,
            commands::messaging::send_typing_indicator,
            commands::messaging::request_link_preview,
            commands::realtime::send_webrtc_signal,
            commands::messaging::send_reaction,
            commands::messaging::pin_message,
            commands::messaging::delete_pchat_messages,
            commands::dm::send_dm,
            commands::dm::get_dm_messages,
            commands::dm::select_dm_user,
            commands::dm::get_dm_unread_counts,
            commands::dm::mark_dm_read,
            commands::system::reset_app_data,
            commands::system::set_log_level,
            commands::system::set_notifications_enabled,
            commands::system::set_disable_dual_path,
            commands::system::update_badge_count,
            commands::system::get_system_clock_format,
            commands::offload::offload_message,
            commands::offload::load_offloaded_message,
            commands::offload::load_offloaded_messages_batch,
            commands::offload::clear_offloaded_messages,
            commands::offload::fetch_older_messages,
            commands::offload::get_debug_stats,
            commands::messaging::super_search,
            commands::messaging::get_photos,
            commands::admin::kick_user,
            commands::admin::ban_user,
            commands::admin::register_user,
            commands::admin::mute_user,
            commands::admin::deafen_user,
            commands::admin::set_priority_speaker,
            commands::admin::reset_user_comment,
            commands::admin::remove_user_avatar,
            commands::admin::move_user_to_channel,
            commands::admin::request_user_stats,
            commands::admin::request_user_list,
            commands::admin::update_user_list,
            commands::admin::request_user_comment,
            commands::admin::request_ban_list,
            commands::admin::update_ban_list,
            commands::admin::request_acl,
            commands::admin::update_acl,
            commands::keyshare::confirm_custodians,
            commands::keyshare::accept_custodian_changes,
            commands::keyshare::approve_key_share,
            commands::keyshare::dismiss_key_share,
            commands::keyshare::query_key_holders,
            commands::keyshare::get_key_holders,
            commands::keyshare::key_takeover,
            commands::image::blur_image,
            commands::image::process_background,
            commands::popout::open_image_popout,
            commands::popout::take_popout_image,
            commands::window::set_window_aspect_ratio,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_check,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_pending,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_download_and_install,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_dismiss,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_set_auto_install,
            #[cfg(not(target_os = "android"))]
            updater::commands::updater_set_skipped_version,
        ]
    };
}

fn register_commands(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder.invoke_handler(all_command_handlers!())
}
