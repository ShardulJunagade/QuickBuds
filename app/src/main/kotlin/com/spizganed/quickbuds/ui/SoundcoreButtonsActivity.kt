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
        val all = (0 until list.length()).map { list.getJSONObject(it) }
        val shape = all.joinToString { key(it) }
        if (shape != built) layout(all, shape)
        for (b in all) rows[key(b)]?.second?.text = actionLabel(b, if (b.isNull("action")) null else b.optInt("action"))
    }

    private fun key(b: JSONObject) = "${b.optString("group")}:${b.optString("press")}:${b.optInt("side")}"

    /**
     * One section per group and earbud: the earbud buttons by side, the call buttons, the slides. A group with
     * one side only (headphones, the function button) gets one section.
     */
    private fun layout(all: List<JSONObject>, shape: String) {
        built = shape
        views.forEach { root.removeView(it) }
        views.clear()
        rows.clear()
        for (group in all.map { it.optString("group") }.distinct()) {
            val items = all.filter { it.optString("group") == group }
            val sides = items.map { it.optInt("side") }.distinct()
            for (side in sides) {
                val label = SettingRowFactory.sectionLabel(this, sectionLabel(group, side, sides.size))
                val card = SettingRowFactory.card(this)
                root.addView(label)
                root.addView(card)
                views += label
                views += card
                for (b in items.filter { it.optInt("side") == side }) {
                    val row = SettingRowFactory.build(this, 0, pressLabel(b.optString("press")), 0, SettingRowFactory.buildChevron(this)) {
                        pick(key(b))
                    }
                    SettingRowFactory.addRow(card, row)
                    rows[key(b)] = row to SettingRowFactory.subtitle(this, row)
                }
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

    private fun sectionLabel(group: String, side: Int, sides: Int): Int = when {
        group == "mfb" -> R.string.sc_group_mfb
        group == "callButtons" && side == 0 -> R.string.sc_group_call_left
        group == "callButtons" -> R.string.sc_group_call_right
        sides == 1 -> R.string.sc_group_buttons
        side == 0 -> R.string.gesture_bud_left
        else -> R.string.gesture_bud_right
    }

    private fun pick(key: String) {
        val s = state ?: return
        val list = s.optJSONArray("buttons") ?: return
        val b = (0 until list.length()).map { list.getJSONObject(it) }.firstOrNull { key(it) == key } ?: return
        val actions = b.optJSONArray("actions")?.let { a -> (0 until a.length()).map { a.getJSONObject(it).getInt("id") } }.orEmpty()
        val current = if (b.isNull("action")) null else b.optInt("action")
        val options: List<Int?> = actions + if (b.optBoolean("canOff")) listOf(null) else emptyList()
        choose(pressLabel(b.optString("press")), options.map { actionLabel(b, it) to (it == current) }) { i ->
            set("button", JSONObject().put("group", b.optString("group")).put("press", b.optString("press"))
                .put("side", b.optInt("side")).put("action", options[i] ?: JSONObject.NULL))
        }
    }

    private fun pressLabel(press: String) = when (press) {
        "single" -> R.string.gesture_single
        "double" -> R.string.gesture_double
        "triple" -> R.string.gesture_triple
        "slideUp" -> R.string.sc_press_slide_up
        "slideDown" -> R.string.sc_press_slide_down
        else -> R.string.gesture_hold
    }

    /** The name of an action id: the model's list names it (`soundcore::models` action lists). */
    private fun actionLabel(b: JSONObject, id: Int?): String {
        if (id == null) return getString(R.string.gesture_action_none)
        val a = b.optJSONArray("actions")
        val name = (0 until (a?.length() ?: 0)).map { a!!.getJSONObject(it) }.firstOrNull { it.optInt("id") == id }?.optString("name")
        return getString(when (name) {
            "volumeUp" -> R.string.gesture_action_volume_up
            "volumeDown" -> R.string.gesture_action_volume_down
            "previous" -> R.string.gesture_action_prev
            "next" -> R.string.gesture_action_next
            "noiseMode" -> R.string.sc_action_noise
            "assistant" -> R.string.gesture_action_assistant
            "playPause" -> R.string.gesture_action_play_pause
            "gaming" -> R.string.gesture_action_game
            "bassUp" -> R.string.sc_action_bass_up
            "lights" -> R.string.sc_action_lights
            "noiseCanceling" -> R.string.sc_mode_anc
            "changeMode" -> R.string.sc_action_change_mode
            "declineCall" -> R.string.gesture_on_call_decline
            "answerCall" -> R.string.sc_action_answer
            "ankaAssistant" -> R.string.sc_action_anka
            "translation" -> R.string.sc_action_translation
            "napMode" -> R.string.sc_action_nap
            else -> R.string.sc_unknown
        })
    }
}
