package com.spizganed.quickbuds.bluetooth

import com.spizganed.quickbuds.bluetooth.BudsDevice.Candidate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BudsDevicePickTest {

    @Test
    fun connectedBudsComeBeforeTheSavedAddress() {
        // Buds 4 saved and in the case, Space A40 connected.
        val buds = listOf(Candidate("BUDS4", false), Candidate("A40", true))
        assertEquals("A40", BudsDevice.pick(buds, saved = "BUDS4"))
    }

    @Test
    fun theSavedAddressWinsAmongConnectedBuds() {
        val buds = listOf(Candidate("A", true), Candidate("B", true))
        assertEquals("B", BudsDevice.pick(buds, saved = "B"))
    }

    @Test
    fun withNothingConnectedTheSavedAddressWins() {
        val buds = listOf(Candidate("A", false), Candidate("B", false))
        assertEquals("B", BudsDevice.pick(buds, saved = "B"))
        assertEquals("A", BudsDevice.pick(buds, saved = null))
    }

    @Test
    fun noBudsGiveNoAddress() {
        assertNull(BudsDevice.pick(emptyList(), saved = "X"))
    }
}
