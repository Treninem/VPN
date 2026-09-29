package ru.amri.vpn

internal data class UnderlyingNetworkChoice<T>(
    val value: T,
    val validated: Boolean,
    val wifi: Boolean,
    val ethernet: Boolean,
    val cellular: Boolean,
)

/** Chooses a real underlying access network and never treats AMRI's VPN as its own upstream. */
internal object AndroidUnderlyingNetworkPolicy {
    fun <T> select(active: T?, choices: List<UnderlyingNetworkChoice<T>>): T? {
        choices.firstOrNull { it.value == active }?.let { return it.value }
        return choices.maxByOrNull { choice ->
            (if (choice.validated) 100 else 0) +
                when {
                    choice.ethernet -> 30
                    choice.wifi -> 20
                    choice.cellular -> 10
                    else -> 0
                }
        }?.value
    }
}
