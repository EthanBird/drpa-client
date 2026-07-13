use drpa_host::HostState;
use drpa_protocol::WorkspaceSnapshot;
use tauri::State;

#[tauri::command]
fn get_workspace_snapshot(state: State<'_, HostState>) -> WorkspaceSnapshot {
    state.snapshot()
}

#[tauri::command]
fn start_run(
    package_id: String,
    profile_id: String,
    state: State<'_, HostState>,
) -> Result<String, String> {
    state
        .start_run(&package_id, &profile_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_run(run_id: String, state: State<'_, HostState>) -> Result<(), String> {
    state.cancel_run(&run_id).map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(HostState::demo())
        .invoke_handler(tauri::generate_handler![
            get_workspace_snapshot,
            start_run,
            cancel_run
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DRPA Next desktop host");
}
