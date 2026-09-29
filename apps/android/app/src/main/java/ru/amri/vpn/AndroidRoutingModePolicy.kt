package ru.amri.vpn

/** Keeps UI routing modes aligned with the runtime candidate policy. */
object AndroidRoutingModePolicy {
    const val SMART = 0
    const val MANUAL = 1

    fun normalize(routingMode: Int): Int = if (routingMode == SMART) SMART else MANUAL

    /**
     * Only Smart mode may use bootstrap ranking/failover. Any legacy non-zero mode is treated as
     * Manual so upgrades never silently enable cross-node failover for a previously explicit mode.
     */
    fun smartRoutingEnabled(routingMode: Int, smartRoutingToggle: Boolean): Boolean =
        normalize(routingMode) == SMART && smartRoutingToggle
}
