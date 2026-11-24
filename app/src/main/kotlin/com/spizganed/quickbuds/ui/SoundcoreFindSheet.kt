package com.spizganed.quickbuds.ui

import android.app.Activity
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import com.spizganed.quickbuds.R
import kotlin.math.PI
import kotlin.math.min
import kotlin.math.sin

/**
 * Find my earbuds for Soundcore buds. The buds have no locator command: the Soundcore app plays the sound
 * from the phone over A2DP (capture 2026-11-24, Space A40). This sheet does the same: a beep on one
 * stereo channel at the media volume. Closing the sheet stops the beep.
 */
class SoundcoreFindSheet(private val activity: Activity) {

    private val sheet = BottomSheetDialog(activity)
    private var track: AudioTrack? = null
    /** The side that plays: 0 left, 1 right, -1 none. */
    private var side = -1

    fun show() {
        sheet.title(activity.getString(R.string.find_title))
            .message(activity.getString(R.string.sc_find_hint))
            .dismissOnSelect(false)
            .onDismiss { stop() }
        render()
        sheet.show()
    }

    private fun render() {
        sheet.items(listOf(R.string.gesture_bud_left, R.string.gesture_bud_right).mapIndexed { i, label ->
            BottomSheetDialog.Item(activity.getString(label), side == i) { play(i) }
        })
        sheet.confirm(activity.getString(R.string.find_stop)) { stop(); render() }
    }

    private fun play(s: Int) {
        stop()
        val pcm = beeps(s)
        track = AudioTrack.Builder()
            .setAudioAttributes(AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION).build())
            .setAudioFormat(AudioFormat.Builder()
                .setEncoding(AudioFormat.ENCODING_PCM_16BIT).setSampleRate(RATE)
                .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build())
            .setTransferMode(AudioTrack.MODE_STATIC)
            .setBufferSizeInBytes(pcm.size * 2)
            .build().apply {
                write(pcm, 0, pcm.size)
                setLoopPoints(0, pcm.size / 2, -1)
                play()
            }
        side = s
        render()
    }

    private fun stop() {
        track?.run { runCatching { stop() }; release() }
        track = null
        side = -1
    }

    private companion object {
        const val RATE = 48_000
        const val HZ = 2_400.0

        /** One 1 s loop: 3 beeps of 120 ms with 80 ms gaps, then silence. Interleaved stereo, one side silent. */
        fun beeps(side: Int): ShortArray {
            val frames = RATE
            val out = ShortArray(frames * 2)
            val beep = RATE * 120 / 1000
            val period = RATE * 200 / 1000
            val fade = RATE * 5 / 1000
            for (b in 0 until 3) for (i in 0 until beep) {
                val env = min(1.0, min(i, beep - 1 - i) / fade.toDouble())
                val v = (sin(2 * PI * HZ * i / RATE) * env * 0.9 * Short.MAX_VALUE).toInt().toShort()
                out[(b * period + i) * 2 + side] = v
            }
            return out
        }
    }
}
