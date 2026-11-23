package com.spizganed.quickbuds.bluetooth

import com.spizganed.quickbuds.protocol.OpoProtocol

/** The Android audio-link state of the paired buds. */
enum class LinkState { CONNECTED, DISCONNECTED, UNKNOWN }

/**
 * The two decisions of an automatic connect. No Android types, so the JVM tests cover both.
 *
 * An automatic connect (unlock, app open, ACL fallback) starts only when Android reports an audio
 * link to the buds. A disconnected link means the buds are in the closed case: they are not ready,
 * and a connect shows 5 failed tries. An unknown state connects as before.
 */
object ConnectPolicy {

    fun autoConnectAllowed(link: LinkState): Boolean = link != LinkState.DISCONNECTED

    fun showTryToast(user: Boolean): Boolean = user

    /**
     * The service UUIDs of one connect, in order. `079A` first: it works on Buds 4, and `1107` burns
     * about 5 s there. Plain SPP comes last, and only when [offered] (the buds' service list) is known
     * and has neither vendor UUID. OnePlus and OPPO buds list `079A`, and their `1101` carries other frames.
     */
    fun connectUuids(offered: List<String>): List<String> {
        val vendor = listOf(OpoProtocol.SPP_UUID_FALLBACK, OpoProtocol.SPP_UUID_PRIMARY)
        val known = offered.map { it.uppercase() }
        val plainOnly = known.isNotEmpty() && vendor.none { it in known }
        return if (plainOnly) vendor + OpoProtocol.SPP_UUID_PLAIN else vendor
    }

    /** Who made the buds: picks the driver before any frame goes out. */
    enum class Brand { OPPO, SOUNDCORE }

    /** Soundcore's vendor service: `0cf12d31-fac3-4553-bd80-d6832e7` plus 5 hex digits (OpenSCQ30, `soundcore.rs`). */
    private const val SOUNDCORE_UUID_PREFIX = "0CF12D31-FAC3-4553-BD80-D6832E7"

    /** Soundcore when the buds list its vendor service, or their name starts with "soundcore". Else OPPO. */
    fun brand(offered: List<String>, name: String?): Brand {
        val vendor = offered.any { it.uppercase().startsWith(SOUNDCORE_UUID_PREFIX) }
        val named = name?.trim()?.startsWith("soundcore", ignoreCase = true) == true
        return if (vendor || named) Brand.SOUNDCORE else Brand.OPPO
    }

    /** The Soundcore service UUIDs of one connect: the vendor ones the buds list, then plain SPP. */
    fun soundcoreUuids(offered: List<String>): List<String> =
        offered.map { it.uppercase() }.filter { it.startsWith(SOUNDCORE_UUID_PREFIX) } + OpoProtocol.SPP_UUID_PLAIN
}

