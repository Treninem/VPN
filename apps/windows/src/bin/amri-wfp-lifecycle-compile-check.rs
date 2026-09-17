#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../wfp_kill_switch.rs"]
mod wfp_kill_switch;
#[path = "../wfp_kill_switch_lifecycle.rs"]
mod wfp_kill_switch_lifecycle;

fn main() {
    let _ = wfp_kill_switch_lifecycle::activate_kill_switch as fn(
        u32,
        &[std::net::IpAddr],
    ) -> Result<wfp_kill_switch::WfpKillSwitch, String>;
}
