package com.spizganed.quickbuds.ui

import android.widget.LinearLayout
import android.widget.TextView
import com.spizganed.quickbuds.R
import org.json.JSONArray
import org.json.JSONObject

/**
 * The gestures of Soundcore buds: one row per press kind and earbud. The model's state lists the presses, and
 * the actions each press can have (`buttons` in docs/SOUNDCORE-PROTOCOL.md), so the screen holds no model table.
 */
class SoundcoreButtonsActivity : SoundcoreScreen() {

    private val rows = mutableMapOf<String, Pair<LinearLayout, TextView>>()
    /** Every view [layout] added, for the next layout. */
    private val views = mutableListOf<android.view.View>()
    private var built: String? = null

    override fun build() {
        root.addView(SettingRowFactory.title(this, R.string.gesture_title))
    }

    /** The rows come from the state's button list: built on the first state, then repainted. */
    override fun render(s: JSONObject) {
        val list = s.optJSONArray("buttons") ?: return
        val shape = (0 until list.length()).joinToString { list.getJSONObject(it).let { b -> "${b.optString("press")}${b.optInt("side")}" } }
        if (shape != built) layout(list, shape)
        for (i in 0 until list.length()) {
            val b = list.getJSONObject(i)
            rows[key(b)]?.second?.text = actionLabel(if (b.isNull("action")) null else b.optInt("action"))
        }
    }

    private fun key(b: JSONObject) = "${b.optString("press")}:${b.optInt("side")}"

    private fun layout(list: JSONArray, shape: String) {
        built = shape
        views.forEach { root.removeView(it) }
        views.clear()
        rows.clear()
        for (side in 0..1) {
            val label = SettingRowFactory.sectionLabel(this, if (side == 0) R.string.gesture_bud_left else R.string.gesture_bud_right)
            val card = SettingRowFactory.card(this)
            root.addView(label)
            root.addView(card)
            views += label
            views += card
            for (i in 0 until list.length()) {
                val b = list.getJSONObject(i)
                if (b.optInt("side") != side) continue
                val row = SettingRowFactory.build(this, 0, pressLabel(b.optString("press")), 0, SettingRowFactory.buildChevron(this)) {
                    pick(key(b))
                }
                SettingRowFactory.addRow(card, row)
                rows[key(b)] = row to SettingRowFactory.subtitle(this, row)
            }
        }
        if (state?.optBoolean("buttonsPaired") == true) root.addView(TextView(this).apply {
            setText(R.string.sc_buttons_pair_note)
            setTextColor(ThemeRes.color(this@SoundcoreButtonsActivity, R.attr.appColorTextSecondary))
            textSize = 13f
            val dp = ThemeRes.dp(this@SoundcoreButtonsActivity, 16f)
            setPadding(dp, dp, dp, 0)
        }.also { views += it })
    }

    private fun pick(key: String) {
        val s = state ?: return
        val list = s.optJSONArray("buttons") ?: return
        val b = (0 until list.length()).map { list.getJSONObject(it) }.firstOrNull { key(it) == key } ?: return
        val actions = b.optJSONArray("actions")?.let { a -> (0 until a.length()).map { a.getInt(it) } }.orEmpty()
        val current = if (b.isNull("action")) null else b.optInt("action")
        val options: List<Int?> = actions + listOf(null)
        choose(pressLabel(b.optString("press")), options.map { actionLabel(it) to (it == current) }) { i ->
            set("button", JSONObject().put("press", b.optString("press")).put("side", b.optInt("side")).put("action", options[i] ?: JSONObject.NULL))
        }
    }

    private fun pressLabel(press: String) = when (press) {
        "single" -> R.string.gesture_single
        "double" -> R.string.gesture_double
        "triple" -> R.string.gesture_triple
        else -> R.string.gesture_hold
    }

    /** The action names of `soundcore::driver::ACTIONS`. */
    private fun actionLabel(id: Int?): String = getString(when (id) {
        0 -> R.string.gesture_action_volume_up
        1 -> R.string.gesture_action_volume_down
        2 -> R.string.gesture_action_prev
        3 -> R.string.gesture_action_next
        4 -> R.string.sc_action_noise
        5 -> R.string.gesture_action_assistant
        6 -> R.string.gesture_action_play_pause
        9 -> R.string.gesture_action_game
        else -> R.string.gesture_action_none
    })
}
