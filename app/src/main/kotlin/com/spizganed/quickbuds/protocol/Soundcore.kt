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

    /**
     * The noise modes of the home noise control: Off, Transparency and noise cancelling, as far as the model has
     * them (the values of the `noise` field: 2 normal, 1 transparency, 0 noise cancelling).
     */
    fun ancModes(c: Context): String {
        val modes = state(c)?.optJSONArray("modes")
        val noise = (0 until (modes?.length() ?: 0)).map { modes!!.getJSONObject(it) }.firstOrNull { it.optString("key") == "noise" }
        val values = noise?.optJSONArray("values")?.let { a -> (0 until a.length()).map { a.getInt(it) } } ?: listOf(0, 1, 2)
        return listOfNotNull(
            """{"mode":1,"slot":0}""".takeIf { 2 in values },
            """{"mode":2,"slot":1}""".takeIf { 1 in values },
            """{"mode":5,"slot":2}""".takeIf { 0 in values },
        ).joinToString(",", "[", "]")
    }

    /** The home rows of a Soundcore model ([RowRules] keys). */
    fun rows(c: Context): Set<String> {
        val out = linkedSetOf(RowRules.EARBUDS)
        val s = state(c) ?: return out
        // A row shows when the model has its setting (docs/SOUNDCORE-PROTOCOL.md section 8).
        val modes = s.optJSONArray("modes")
        if (modes != null && (0 until modes.length()).any { modes.getJSONObject(it).optString("key") == "noise" }) out += RowRules.ANC
        if (s.has("gaming")) out += RowRules.GAME
        if (s.has("ldac")) out += RowRules.HIRES
        if (s.has("eqPresets")) out += RowRules.EQ
        if (s.has("dual")) out += RowRules.DUAL
        return out
    }

    /** The models.json-style name, for the model line and reports. */
    fun name(c: Context): String? = state(c)?.optString("name")?.takeIf { it.isNotEmpty() }
}
