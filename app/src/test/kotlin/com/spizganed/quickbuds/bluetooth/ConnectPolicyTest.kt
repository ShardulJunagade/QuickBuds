package com.spizganed.quickbuds.bluetooth

import com.spizganed.quickbuds.protocol.OpoProtocol
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Automatic connects (unlock, app open, ACL fallback) start only when Android reports an audio
 * link to the buds. The user's own connect keeps the 5 try toasts; an automatic one is silent.
 */
class ConnectPolicyTest {

    @Test fun automaticConnectStartsWithTheAudioLink() {
        assertTrue(ConnectPolicy.autoConnectAllowed(LinkState.CONNECTED))
    }

    @Test fun automaticConnectDoesNotStartWithoutTheAudioLink() {
        assertFalse(ConnectPolicy.autoConnectAllowed(LinkState.DISCONNECTED))
    }

    @Test fun anUnknownLinkStateConnectsAsToday() {
        assertTrue(ConnectPolicy.autoConnectAllowed(LinkState.UNKNOWN))
    }

    @Test fun theUserConnectShowsTheTryToasts() {
        assertTrue(ConnectPolicy.showTryToast(user = true))
    }

    @Test fun anAutomaticConnectIsSilent() {
        assertFalse(ConnectPolicy.showTryToast(user = false))
    }

    private val vendorFirst = listOf(OpoProtocol.SPP_UUID_FALLBACK, OpoProtocol.SPP_UUID_PRIMARY)

    @Test fun budsWithTheVendorServiceNeverTryPlainSpp() {
        val offered = listOf("0000110b-0000-1000-8000-00805f9b34fb", "0000079a-d102-11e1-9b23-00025b00a5a5", "00001101-0000-1000-8000-00805f9b34fb")
        assertEquals(vendorFirst, ConnectPolicy.connectUuids(offered))
    }

    @Test fun anUnknownServiceListNeverTriesPlainSpp() {
        assertEquals(vendorFirst, ConnectPolicy.connectUuids(emptyList()))
    }

    @Test fun budsWithoutTheVendorServiceTryPlainSppLast() {
        val offered = listOf("0000110b-0000-1000-8000-00805f9b34fb", "00001101-0000-1000-8000-00805f9b34fb")
        assertEquals(vendorFirst + OpoProtocol.SPP_UUID_PLAIN, ConnectPolicy.connectUuids(offered))
    }

    @Test fun soundcoreBudsAreFoundByTheirServiceOrName() {
        val sc = "0cf12d31-fac3-4553-bd80-d6832e7abcde"
        assertEquals(ConnectPolicy.Brand.SOUNDCORE, ConnectPolicy.brand(listOf(sc), "Liberty 5"))
        assertEquals(ConnectPolicy.Brand.SOUNDCORE, ConnectPolicy.brand(emptyList(), "soundcore Space A40"))
        assertEquals(ConnectPolicy.Brand.OPPO, ConnectPolicy.brand(listOf(OpoProtocol.SPP_UUID_PRIMARY), "OnePlus Buds 4"))
        assertEquals(listOf(sc.uppercase(), OpoProtocol.SPP_UUID_PLAIN), ConnectPolicy.soundcoreUuids(listOf(sc, OpoProtocol.SPP_UUID_PRIMARY)))
    }
}
