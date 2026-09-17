use std::net::IpAddr;
use std::ptr::{null, null_mut};
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::{
    FwpmEngineClose0, FwpmEngineOpen0, FwpmFilterAdd0, FwpmSubLayerAdd0,
    FwpmTransactionAbort0, FwpmTransactionBegin0, FwpmTransactionCommit0,
    FWP_ACTION_BLOCK, FWP_ACTION_PERMIT, FWP_CONDITION_FLAG_IS_LOOPBACK,
    FWP_CONDITION_VALUE0, FWP_CONDITION_VALUE0_0, FWP_MATCH_EQUAL, FWP_MATCH_FLAGS_ALL_SET,
    FWP_UINT16, FWP_UINT32, FWP_UINT8, FWP_V4_ADDR_AND_MASK, FWP_V4_ADDR_MASK,
    FWP_V6_ADDR_AND_MASK, FWP_V6_ADDR_MASK, FWPM_CONDITION_FLAGS, FWPM_CONDITION_INTERFACE_INDEX,
    FWPM_CONDITION_IP_LOCAL_PORT, FWPM_CONDITION_IP_PROTOCOL, FWPM_CONDITION_IP_REMOTE_ADDRESS,
    FWPM_CONDITION_IP_REMOTE_PORT, FWPM_FILTER0, FWPM_FILTER_CONDITION0,
    FWPM_LAYER_ALE_AUTH_CONNECT_V4, FWPM_LAYER_ALE_AUTH_CONNECT_V6, FWPM_SESSION0,
    FWPM_SESSION_FLAG_DYNAMIC, FWPM_SUBLAYER0,
};
use windows_sys::Win32::System::Rpc::RPC_C_AUTHN_DEFAULT;

const AMRI_WFP_SUBLAYER: GUID = GUID::from_u128(0x81ef0eea_527b_4aa4_a19d_430798fc9b36);
const AMRI_WFP_SUBLAYER_WEIGHT: u16 = 0xF000;
const PERMIT_WEIGHT: u8 = 15;
const BLOCK_WEIGHT: u8 = 0;
const IPPROTO_UDP: u8 = 17;

/// Owns a dynamic Windows Filtering Platform session.
///
/// All AMRI filters and the custom sublayer are session-scoped. Closing this handle, including
/// during process teardown, removes them from BFE. The handle is transferred once from the
/// forwarding worker to its owner and is never concurrently used after setup.
pub(crate) struct WfpKillSwitch {
    engine: HANDLE,
}

// SAFETY: a WFP engine handle is an opaque process handle. AMRI transfers sole ownership once
// after setup and only closes it from the receiving owner; no WFP call races with that close.
unsafe impl Send for WfpKillSwitch {}

impl WfpKillSwitch {
    pub(crate) fn activate(tun_index: u32, bypass_ips: &[IpAddr]) -> Result<Self, String> {
        validate_policy_inputs(tun_index, bypass_ips)?;

        let mut session = FWPM_SESSION0::default();
        session.flags = FWPM_SESSION_FLAG_DYNAMIC;
        let mut engine: HANDLE = null_mut();
        let status = unsafe {
            FwpmEngineOpen0(
                null(),
                RPC_C_AUTHN_DEFAULT as u32,
                null(),
                &session,
                &mut engine,
            )
        };
        if status != 0 || engine.is_null() {
            return Err(wfp_error("failed to open dynamic WFP session", status));
        }

        let result = install_policy(engine, tun_index, bypass_ips);
        if let Err(error) = result {
            unsafe {
                FwpmEngineClose0(engine);
            }
            return Err(error);
        }

        Ok(Self { engine })
    }
}

impl Drop for WfpKillSwitch {
    fn drop(&mut self) {
        if !self.engine.is_null() {
            unsafe {
                FwpmEngineClose0(self.engine);
            }
            self.engine = null_mut();
        }
    }
}

fn validate_policy_inputs(tun_index: u32, bypass_ips: &[IpAddr]) -> Result<(), String> {
    if tun_index == 0 {
        return Err("WFP kill switch requires a valid AMRI TUN interface index".into());
    }
    if bypass_ips.is_empty() {
        return Err("WFP kill switch requires at least one pinned VPN endpoint address".into());
    }
    Ok(())
}

fn install_policy(engine: HANDLE, tun_index: u32, bypass_ips: &[IpAddr]) -> Result<(), String> {
    let begin_status = unsafe { FwpmTransactionBegin0(engine, 0) };
    if begin_status != 0 {
        return Err(wfp_error("failed to begin WFP transaction", begin_status));
    }

    let result = (|| {
        add_sublayer(engine)?;

        // Traffic already routed to AMRI's Wintun adapter is allowed. Direct traffic using a
        // physical interface falls through to the final block rules below.
        add_interface_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V4, tun_index, "IPv4")?;
        add_interface_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V6, tun_index, "IPv6")?;

        // tun2proxy and sing-box communicate over loopback; loopback must never be caught by the
        // direct-egress block.
        add_loopback_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V4, "IPv4")?;
        add_loopback_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V6, "IPv6")?;

        // The encrypted transport itself must be able to reach only the endpoint addresses that
        // were resolved and pinned before default-route capture. No DNS exception is required.
        for ip in bypass_ips {
            match ip {
                IpAddr::V4(ip) => add_ipv4_endpoint_permit(engine, *ip)?,
                IpAddr::V6(ip) => add_ipv6_endpoint_permit(engine, *ip)?,
            }
        }

        // Preserve lease renewal while the kill switch is active. These narrow UDP exceptions are
        // restricted to the standard client/server DHCP port pairs.
        add_dhcp_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V4, 68, 67, "DHCPv4")?;
        add_dhcp_permit(engine, FWPM_LAYER_ALE_AUTH_CONNECT_V6, 546, 547, "DHCPv6")?;

        // Hard block is intentionally last and lower-weight in the same AMRI sublayer. Matching
        // permit filters above stop evaluation of this sublayer before the catch-all block.
        add_filter(
            engine,
            FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            FWP_ACTION_BLOCK,
            BLOCK_WEIGHT,
            &mut [],
            "AMRI Kill Switch block direct IPv4",
        )?;
        add_filter(
            engine,
            FWPM_LAYER_ALE_AUTH_CONNECT_V6,
            FWP_ACTION_BLOCK,
            BLOCK_WEIGHT,
            &mut [],
            "AMRI Kill Switch block direct IPv6",
        )?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            let commit_status = unsafe { FwpmTransactionCommit0(engine) };
            if commit_status == 0 {
                Ok(())
            } else {
                let _ = unsafe { FwpmTransactionAbort0(engine) };
                Err(wfp_error("failed to commit WFP kill switch", commit_status))
            }
        }
        Err(error) => {
            let _ = unsafe { FwpmTransactionAbort0(engine) };
            Err(error)
        }
    }
}

fn add_sublayer(engine: HANDLE) -> Result<(), String> {
    let mut name = wide("AMRI VPN Kill Switch");
    let mut description = wide("Dynamic AMRI fail-closed outbound filtering");
    let mut sublayer = FWPM_SUBLAYER0::default();
    sublayer.subLayerKey = AMRI_WFP_SUBLAYER;
    sublayer.displayData.name = name.as_mut_ptr();
    sublayer.displayData.description = description.as_mut_ptr();
    sublayer.weight = AMRI_WFP_SUBLAYER_WEIGHT;

    let status = unsafe { FwpmSubLayerAdd0(engine, &sublayer, null_mut()) };
    if status == 0 {
        Ok(())
    } else {
        Err(wfp_error("failed to add AMRI WFP sublayer", status))
    }
}

fn add_interface_permit(
    engine: HANDLE,
    layer: GUID,
    tun_index: u32,
    family: &str,
) -> Result<(), String> {
    let mut conditions = [uint32_condition(
        FWPM_CONDITION_INTERFACE_INDEX,
        FWP_MATCH_EQUAL,
        tun_index,
    )];
    add_filter(
        engine,
        layer,
        FWP_ACTION_PERMIT,
        PERMIT_WEIGHT,
        &mut conditions,
        &format!("AMRI permit {family} through TUN"),
    )
}

fn add_loopback_permit(engine: HANDLE, layer: GUID, family: &str) -> Result<(), String> {
    let mut conditions = [uint32_condition(
        FWPM_CONDITION_FLAGS,
        FWP_MATCH_FLAGS_ALL_SET,
        FWP_CONDITION_FLAG_IS_LOOPBACK,
    )];
    add_filter(
        engine,
        layer,
        FWP_ACTION_PERMIT,
        PERMIT_WEIGHT,
        &mut conditions,
        &format!("AMRI permit {family} loopback"),
    )
}

fn add_ipv4_endpoint_permit(engine: HANDLE, ip: std::net::Ipv4Addr) -> Result<(), String> {
    let mut address = FWP_V4_ADDR_AND_MASK {
        addr: u32::from_be_bytes(ip.octets()),
        mask: u32::MAX,
    };
    let mut condition = FWPM_FILTER_CONDITION0::default();
    condition.fieldKey = FWPM_CONDITION_IP_REMOTE_ADDRESS;
    condition.matchType = FWP_MATCH_EQUAL;
    condition.conditionValue.r#type = FWP_V4_ADDR_MASK;
    condition.conditionValue.Anonymous = FWP_CONDITION_VALUE0_0 {
        v4AddrMask: &mut address,
    };
    add_filter(
        engine,
        FWPM_LAYER_ALE_AUTH_CONNECT_V4,
        FWP_ACTION_PERMIT,
        PERMIT_WEIGHT,
        std::slice::from_mut(&mut condition),
        "AMRI permit pinned IPv4 VPN endpoint",
    )
}

fn add_ipv6_endpoint_permit(engine: HANDLE, ip: std::net::Ipv6Addr) -> Result<(), String> {
    let mut address = FWP_V6_ADDR_AND_MASK {
        addr: ip.octets(),
        prefixLength: 128,
    };
    let mut condition = FWPM_FILTER_CONDITION0::default();
    condition.fieldKey = FWPM_CONDITION_IP_REMOTE_ADDRESS;
    condition.matchType = FWP_MATCH_EQUAL;
    condition.conditionValue.r#type = FWP_V6_ADDR_MASK;
    condition.conditionValue.Anonymous = FWP_CONDITION_VALUE0_0 {
        v6AddrMask: &mut address,
    };
    add_filter(
        engine,
        FWPM_LAYER_ALE_AUTH_CONNECT_V6,
        FWP_ACTION_PERMIT,
        PERMIT_WEIGHT,
        std::slice::from_mut(&mut condition),
        "AMRI permit pinned IPv6 VPN endpoint",
    )
}

fn add_dhcp_permit(
    engine: HANDLE,
    layer: GUID,
    local_port: u16,
    remote_port: u16,
    label: &str,
) -> Result<(), String> {
    let mut conditions = [
        uint8_condition(FWPM_CONDITION_IP_PROTOCOL, FWP_MATCH_EQUAL, IPPROTO_UDP),
        uint16_condition(FWPM_CONDITION_IP_LOCAL_PORT, FWP_MATCH_EQUAL, local_port),
        uint16_condition(FWPM_CONDITION_IP_REMOTE_PORT, FWP_MATCH_EQUAL, remote_port),
    ];
    add_filter(
        engine,
        layer,
        FWP_ACTION_PERMIT,
        PERMIT_WEIGHT,
        &mut conditions,
        &format!("AMRI permit {label} lease renewal"),
    )
}

fn add_filter(
    engine: HANDLE,
    layer: GUID,
    action: u32,
    weight: u8,
    conditions: &mut [FWPM_FILTER_CONDITION0],
    label: &str,
) -> Result<(), String> {
    let mut name = wide(label);
    let mut description = wide("AMRI VPN dynamic kill switch rule");
    let mut filter = FWPM_FILTER0::default();
    filter.displayData.name = name.as_mut_ptr();
    filter.displayData.description = description.as_mut_ptr();
    filter.layerKey = layer;
    filter.subLayerKey = AMRI_WFP_SUBLAYER;
    filter.weight.r#type = FWP_UINT8;
    filter.weight.Anonymous.uint8 = weight;
    filter.action.r#type = action;
    filter.numFilterConditions = conditions.len() as u32;
    filter.filterCondition = if conditions.is_empty() {
        null_mut()
    } else {
        conditions.as_mut_ptr()
    };

    let status = unsafe { FwpmFilterAdd0(engine, &filter, null_mut(), null_mut()) };
    if status == 0 {
        Ok(())
    } else {
        Err(wfp_error(label, status))
    }
}

fn uint8_condition(field: GUID, match_type: i32, value: u8) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT8,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint8: value },
        },
    }
}

fn uint16_condition(field: GUID, match_type: i32, value: u16) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT16,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint16: value },
        },
    }
}

fn uint32_condition(field: GUID, match_type: i32, value: u32) -> FWPM_FILTER_CONDITION0 {
    FWPM_FILTER_CONDITION0 {
        fieldKey: field,
        matchType: match_type,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_UINT32,
            Anonymous: FWP_CONDITION_VALUE0_0 { uint32: value },
        },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wfp_error(context: &str, status: u32) -> String {
    format!("{context} (WFP status 0x{status:08X})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_rejects_missing_tun_or_endpoint() {
        let endpoint: IpAddr = "203.0.113.7".parse().unwrap();
        assert!(validate_policy_inputs(0, &[endpoint]).is_err());
        assert!(validate_policy_inputs(7, &[]).is_err());
        assert!(validate_policy_inputs(7, &[endpoint]).is_ok());
    }

    #[test]
    fn pinned_ipv4_is_encoded_in_host_order() {
        let ip: std::net::Ipv4Addr = "203.0.113.7".parse().unwrap();
        assert_eq!(u32::from_be_bytes(ip.octets()), 0xCB00_7107);
    }
}
