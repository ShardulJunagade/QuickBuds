package com.spizganed.quickbuds.protocol

import org.junit.Assert.assertEquals
import org.junit.Test

/** The log lines of both engines decode; a bracket in the text after the hex is no label. */
class LogDecoderTest {
    @Test fun rustEngineLinesWithBracketsInTheirTextDecode() {
        val rx = LogDecoder.decode("22:58:43.612  RX: AA 09 00 00 14 81 0A 02 00 00 08 (bitmap [0100 0103])")
        assertEquals(LogDecoder.Direction.RX, rx.direction)
        val tx = LogDecoder.decode("22:58:43.579  TX: AA 07 00 00 14 01 0A 00 00 (Query codec)")
        assertEquals(LogDecoder.Direction.TX, tx.direction)
    }

    @Test fun kotlinEngineLinesStillDecode() {
        assertEquals(LogDecoder.Direction.TX, LogDecoder.decode("22:58:43.579  TX[query codec]: AA 07 00 00 14 01 0A 00 00").direction)
        assertEquals(LogDecoder.Direction.RX, LogDecoder.decode("22:58:43.612  RX[batch]: AA 09 00 00 14 81 0A 02 00 00 08").direction)
    }
}
