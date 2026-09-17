package ru.amri.vpn

/** Runtime support matrix used by the UI so reserved controls cannot imply active protection. */
object AndroidFeatureAvailability {
    const val DNS_POLICY_TOGGLE = false
    const val APP_KILL_SWITCH_TOGGLE = false
    const val LOCAL_LEARNING = false
    const val SHARED_LEARNING = false
}
