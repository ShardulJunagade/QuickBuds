package com.spizganed.quickbuds.protocol

import android.content.Context
import com.spizganed.quickbuds.ui.ThemeRes
import org.json.JSONObject

/**
 * The Soundcore buds of the current or last link. The core's Soundcore driver gives every setting as one JSON
 * object (`core/src/soundcore/driver.rs` `state_json`, docs/SOUNDCORE-PROTOCOL.md). This object keeps the last one
 * on disk, so the home screen, the widget and the tiles know the brand and the rows after a restart.
 */
object Soundcore {
    private const val KEY_ACTIVE = "soundcoreActive"
    private const val KEY_STATE = "soundcoreState"

    @Volatile private var cached: JSONObject? = null

    private fun prefs(c: Context) = c.getSharedPreferences(ThemeRes.PREFS_NAME, Context.MODE_PRIVATE)

    /** The current or last link is a Soundcore link. */
    fun active(c: Context): Boolean = prefs(c).getBoolean(KEY_ACTIVE, false)

    /** A connect picked its brand ([com.spizganed.quickbuds.bluetooth.ConnectPolicy.brand]). */
    fun setActive(c: Context, on: Boolean) {
        if (active(c) != on) prefs(c).edit().putBoolean(KEY_ACTIVE, on).apply()
    }

    /** The last Soundcore state, or null before the first state packet. */
    fun state(c: Context): JSONObject? = cached ?: prefs(c).getString(KEY_STATE, null)
        ?.let { runCatching { JSONObject(it) }.getOrNull() }?.also { cached = it }

    fun save(c: Context, state: JSONObject) {
        cached = state
        prefs(c).edit().putString(KEY_STATE, state.toString()).apply()
    }

    /** The noise modes of every Soundcore model this app knows: Off, Transparency and noise cancelling. */
    val ANC_MODES = """[{"mode":1,"slot":0},{"mode":2,"slot":1},{"mode":5,"slot":2}]"""

    /** The home rows of a Soundcore model ([RowRules] keys). */
    fun rows(c: Context): Set<String> {
        val out = linkedSetOf(RowRules.EARBUDS)
        if (state(c) == null) return out
        out += listOf(RowRules.ANC, RowRules.GAME, RowRules.HIRES, RowRules.EQ, RowRules.DUAL)
        return out
    }

    /** The models.json-style name, for the model line and reports. */
    fun name(c: Context): String? = state(c)?.optString("name")?.takeIf { it.isNotEmpty() }
}
