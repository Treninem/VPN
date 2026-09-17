use std::net::IpAddr;

use crate::wfp_kill_switch::WfpKillSwitch;

/// Activates the Windows Filtering Platform guard only after AMRI has a concrete TUN interface
/// and pinned transport endpoints. Keeping the returned guard alive keeps the fail-closed policy
/// active; dropping it closes the dynamic WFP session and removes all AMRI filters automatically.
pub(crate) fn activate_kill_switch(
    tun_index: u32,
    bypass_ips: &[IpAddr],
) -> Result<WfpKillSwitch, String> {
    WfpKillSwitch::activate(tun_index, bypass_ips)
}
