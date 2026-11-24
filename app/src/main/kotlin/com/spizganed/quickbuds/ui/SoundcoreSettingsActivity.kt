package com.spizganed.quickbuds.ui

import android.content.Intent
import android.view.View
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import com.spizganed.quickbuds.R
import org.json.JSONArray
import org.json.JSONObject

/**
 * Earbud settings of Soundcore buds. The core gives each model's settings as lists (`modes`, `switches`,
 * `choices`; docs/SOUNDCORE-PROTOCOL.md section 8), so one screen serves every model: a row shows only when
 * its key is in the state, and a noise control row only while its parent mode is on (`shown`).
 */
class SoundcoreSettingsActivity : SoundcoreScreen() {

    /** A sound mode field: its title, and for a switch its subtitle. Order = screen order. */
    private class ModeRow(val key: String, val title: Int, val sub: Int = 0, val switch: Boolean = false)

    private val modeRows = listOf(
        ModeRow("transparencyMode", R.string.sc_transparency_title),
        ModeRow("transparencyLevel", R.string.sc_transparency_level_title),
        ModeRow("ancMode", R.string.sc_anc_mode_title),
        ModeRow("ancLevel", R.string.sc_anc_level_title),
        ModeRow("customAnc", R.string.sc_anc_level_title),
        ModeRow("adaptiveLevel", R.string.sc_adaptive_title),
        ModeRow("sensitivity", R.string.sc_sensitivity_title),
        ModeRow("environmentDetection", R.string.sc_environment_title, R.string.sc_environment_sub, switch = true),
        ModeRow("realTimeAdaptive", R.string.sc_realtime_title, R.string.sc_realtime_sub, switch = true),
        ModeRow("transport", R.string.sc_transport_title),
        ModeRow("multiScene", R.string.sc_multi_scene_title),
        ModeRow("airplane", R.string.sc_airplane_title),
        ModeRow("windNoise", R.string.row_wind_noise_title, R.string.sc_wind_sub, switch = true),
    )

    /** The one-byte switches: key, title, subtitle. Order = screen order. */
    private val switchKeys = listOf(
        Triple("touchTone", R.string.sc_touch_tone_title, R.string.sc_touch_tone_sub),
        Triple("wearingTone", R.string.sc_wearing_tone_title, R.string.sc_wearing_tone_sub),
        Triple("lowBattery", R.string.sc_low_battery_title, R.string.sc_low_battery_sub),
        Triple("voicePrompt", R.string.sc_voice_prompt_title, R.string.sc_voice_prompt_sub),
        Triple("ambientPrompt", R.string.sc_ambient_prompt_title, R.string.sc_ambient_prompt_sub),
        Triple("wearDetection", R.string.row_wear_title, R.string.row_wear_sub),
        Triple("autoPlay", R.string.sc_auto_play_title, R.string.sc_auto_play_sub),
        Triple("leak", R.string.sc_leak_title, R.string.sc_leak_sub),
        Triple("surround", R.string.sc_surround_title, R.string.sc_surround_sub),
        Triple("dolby", R.string.sc_dolby_title, R.string.sc_dolby_sub),
        Triple("sideTone", R.string.sc_side_tone_title, R.string.sc_side_tone_sub),
        Triple("personalizedAnc", R.string.sc_personal_anc_title, R.string.sc_personal_anc_sub),
        Triple("touchLock", R.string.sc_touch_lock_title, R.string.sc_touch_lock_sub),
        Triple("buttonsOff", R.string.sc_controls_on_title, R.string.sc_controls_on_sub),
        Triple("tapOff", R.string.sc_controls_on_title, R.string.sc_controls_on_sub),
        Triple("heyAnka", R.string.sc_hey_anka_title, R.string.sc_hey_anka_sub),
        Triple("sleepAnc", R.string.sc_sleep_anc_title, R.string.sc_sleep_anc_sub),
        Triple("incomingCalls", R.string.sc_incoming_calls_title, R.string.sc_incoming_calls_sub),
        Triple("autoSwitchAsleep", R.string.sc_auto_switch_title, R.string.sc_auto_switch_sub),
        Triple("autoOffPrompt", R.string.sc_auto_off_prompt_title, R.string.sc_auto_off_prompt_sub),
        Triple("listeningModePrompt", R.string.sc_mode_prompt_title, R.string.sc_mode_prompt_sub),
    )

    /** The one-byte choices: key, title. */
    private val choiceKeys = listOf(
        "pressSensitivity" to R.string.sc_press_title,
        "volumeBalance" to R.string.sc_balance_title,
        "listeningMode" to R.string.sc_listening_title,
        "defaultListeningMode" to R.string.sc_default_listening_title,
        "caseLanguage" to R.string.sc_case_language_title,
    )

    private val modes = mutableMapOf<String, Pair<LinearLayout, View>>()
    private val switches = mutableMapOf<String, Pair<LinearLayout, Switch>>()
    private val choices = mutableMapOf<String, Pair<LinearLayout, TextView>>()
    private lateinit var gestures: LinearLayout
    private lateinit var cycle: Pair<LinearLayout, TextView>
    private lateinit var reset: LinearLayout
    private lateinit var autoOff: Pair<LinearLayout, TextView>
    private lateinit var limit: Pair<LinearLayout, Switch>
    private lateinit var limitDb: Pair<LinearLayout, TextView>
    private lateinit var limitRate: Pair<LinearLayout, TextView>
    private lateinit var firmware: Pair<LinearLayout, TextView>
    private lateinit var serial: Pair<LinearLayout, TextView>
    private lateinit var noiseLabel: View
    private lateinit var noiseHint: View
    private lateinit var noiseCard: LinearLayout
    private lateinit var controlsLabel: View
    private lateinit var controlsCard: LinearLayout
    private lateinit var budsCard: LinearLayout
    private lateinit var volumeLabel: View
    private lateinit var volumeCard: LinearLayout

    override fun build() {
        root.addView(SettingRowFactory.title(this, R.string.row_earbuds_title))

        noiseLabel = section(R.string.anc_section)
        noiseHint = TextView(this).apply {
            setText(R.string.sc_noise_hint)
            setTextColor(ThemeRes.color(this@SoundcoreSettingsActivity, R.attr.appColorTextSecondary))
            textSize = 13f
            val dp = ThemeRes.dp(this@SoundcoreSettingsActivity, 16f)
            setPadding(dp, 0, dp, dp / 2)
        }
        root.addView(noiseHint)
        noiseCard = card()
        for (m in modeRows) {
            modes[m.key] = if (m.switch) switchRow(noiseCard, m.title, m.sub, m.key)
            else if (m.key == "adaptiveLevel") SettingRowFactory.build(this, 0, m.title, 0, null).let { row ->
                SettingRowFactory.addRow(noiseCard, row); row to SettingRowFactory.subtitle(this, row)
            }
            else choiceRow(noiseCard, m.title) { pickMode(m.key, m.title) }
        }

        controlsLabel = section(R.string.sc_section_controls)
        controlsCard = card()
        gestures = SettingRowFactory.build(this, 0, R.string.gesture_title, R.string.sc_buttons_sub,
            SettingRowFactory.buildChevron(this)) { startActivity(Intent(this, SoundcoreButtonsActivity::class.java)) }
        SettingRowFactory.addRow(controlsCard, gestures)
        cycle = choiceRow(controlsCard, R.string.sc_cycle_title) { pickCycle() }
        reset = SettingRowFactory.build(this, 0, R.string.sc_reset_buttons_title, R.string.sc_reset_buttons_sub, null) { confirmReset() }
        SettingRowFactory.addRow(controlsCard, reset)

        section(R.string.sc_section_earbuds)
        budsCard = card()
        for ((key, title, sub) in switchKeys) switches[key] = switchRow(budsCard, title, sub, key)
        for ((key, title) in choiceKeys) choices[key] = choiceRow(budsCard, title) { pickChoice(key, title) }
        autoOff = choiceRow(budsCard, R.string.sc_auto_off_title) { pickAutoOff() }

        volumeLabel = section(R.string.sc_section_volume)
        volumeCard = card()
        limit = SettingRowFactory.buildSwitch(this, false).let { sw ->
            sw.setOnCheckedChangeListener { _, on -> if (!painting) set("volumeLimit", JSONObject().put("on", on)) }
            val row = SettingRowFactory.build(this, 0, R.string.sc_volume_limit_title, R.string.sc_volume_limit_sub, sw) { sw.performClick() }
            SettingRowFactory.addRow(volumeCard, row)
            row to sw
        }
        limitDb = choiceRow(volumeCard, R.string.sc_volume_limit_db_title) { pickLimitDb() }
        limitRate = choiceRow(volumeCard, R.string.sc_refresh_title) { pickLimitRate() }

        section(R.string.sc_section_about)
        val about = card()
        firmware = SettingRowFactory.build(this, 0, R.string.row_firmware_title, 0, null).let { row ->
            SettingRowFactory.addRow(about, row); row to SettingRowFactory.subtitle(this, row)
        }
        serial = SettingRowFactory.build(this, 0, R.string.sc_serial_title, 0, null).let { row ->
            SettingRowFactory.addRow(about, row); row to SettingRowFactory.subtitle(this, row)
        }
    }

    private fun show(row: View, on: Boolean) { row.visibility = if (on) View.VISIBLE else View.GONE }

    /** A divider ([SettingRowFactory.addRow]) shows only between two shown rows. */
    private fun tidy(card: LinearLayout) {
        var rowBefore = false
        for (i in 0 until card.childCount) {
            val v = card.getChildAt(i)
            if (v is LinearLayout) { if (v.visibility == View.VISIBLE) rowBefore = true; continue }
            val next = (i + 1 until card.childCount).map { card.getChildAt(it) }.firstOrNull { it is LinearLayout }
            v.visibility = if (rowBefore && next?.visibility == View.VISIBLE) View.VISIBLE else View.GONE
        }
    }

    private fun list(a: JSONArray?): List<JSONObject> = (0 until (a?.length() ?: 0)).map { a!!.getJSONObject(it) }

    private fun mode(s: JSONObject, key: String): JSONObject? = list(s.optJSONArray("modes")).firstOrNull { it.optString("key") == key }

    /** The text of a value: its name ([valueLabel]), else its number. */
    private fun label(field: JSONObject, value: Int): String =
        field.optJSONObject("labels")?.optString(value.toString())?.takeIf { it.isNotEmpty() }?.let { valueLabel(it) }
            ?: getString(R.string.sc_level_n, value)

    private fun valueLabel(name: String): String = getString(when (name) {
        "transport" -> R.string.sc_anc_transport
        "outdoor" -> R.string.sc_anc_outdoor
        "indoor" -> R.string.sc_anc_indoor
        "custom" -> R.string.sc_anc_custom
        "manual" -> R.string.sc_anc_manual
        "adaptive" -> R.string.sc_anc_adaptive
        "multiScene" -> R.string.sc_anc_multi_scene
        "full" -> R.string.sc_transparency_full
        "vocal" -> R.string.sc_transparency_vocal
        "talk" -> R.string.sc_transparency_talk
        "plane" -> R.string.sc_transport_plane
        "train" -> R.string.sc_transport_train
        "bus" -> R.string.sc_transport_bus
        "car" -> R.string.sc_transport_car
        "weak" -> R.string.sc_level_weak
        "moderate" -> R.string.sc_level_moderate
        "strong" -> R.string.sc_level_strong
        "lowNoise" -> R.string.sc_noise_low
        "mediumNoise" -> R.string.sc_noise_medium
        "highNoise" -> R.string.sc_noise_high
        "manualUpdate" -> R.string.sc_airplane_manual
        "automaticUpdate" -> R.string.sc_airplane_auto
        else -> R.string.sc_unknown
    })

    /** The text of a choice value. */
    private fun choiceLabel(key: String, v: Int): String = when (key) {
        "pressSensitivity" -> getString(when (v) { 0 -> R.string.sc_press_soft; 2 -> R.string.sc_press_firm; else -> R.string.sc_press_medium })
        "listeningMode", "defaultListeningMode" -> getString(if (v == 1) R.string.sc_listening_local else R.string.sc_listening_bluetooth)
        "caseLanguage" -> getString(when (v) { 0 -> R.string.sc_lang_japanese; 2 -> R.string.sc_lang_chinese; else -> R.string.sc_lang_english })
        // 0 to 12, 6 is the middle: left below, right above.
        "volumeBalance" -> when {
            v < 6 -> getString(R.string.sc_balance_left, 6 - v)
            v > 6 -> getString(R.string.sc_balance_right, v - 6)
            else -> getString(R.string.sc_balance_center)
        }
        else -> v.toString()
    }

    override fun render(s: JSONObject) {
        var anyMode = false
        for (m in modeRows) {
            val (row, view) = modes.getValue(m.key)
            val f = mode(s, m.key)
            val on = f != null && f.optBoolean("shown")
            show(row, on)
            if (!on || f == null) continue
            anyMode = true
            val v = f.optInt("value")
            when (view) {
                is Switch -> view.paint(v != 0)
                is TextView -> view.text = label(f, v)
            }
        }
        modes["windNoise"]?.first?.let { row ->
            val detected = mode(s, "windDetected")?.optInt("value") == 1
            SettingRowFactory.subtitle(this, row).setText(if (detected) R.string.sc_wind_detected else R.string.sc_wind_sub)
        }
        val hasModes = s.has("modes") && list(s.optJSONArray("modes")).any { it.optString("key") != "noise" }
        show(noiseLabel, hasModes)
        show(noiseCard, anyMode)
        show(noiseHint, hasModes && !anyMode)
        tidy(noiseCard)

        val c = s.optJSONObject("cycle")
        show(gestures, s.has("buttons"))
        show(cycle.first, c != null)
        show(reset, s.optBoolean("buttonsReset"))
        cycle.second.text = listOfNotNull(
            getString(R.string.sc_mode_anc).takeIf { c?.optBoolean("anc") == true },
            getString(R.string.anc_seg_trans).takeIf { c?.optBoolean("transparency") == true },
            getString(R.string.sc_mode_normal).takeIf { c?.optBoolean("normal") == true },
        ).joinToString(" · ")
        val controls = s.has("buttons") || c != null
        show(controlsLabel, controls)
        show(controlsCard, controls)
        tidy(controlsCard)

        val sw = list(s.optJSONArray("switches")).associate { it.optString("key") to it.optBoolean("on") }
        for ((key, pair) in switches) {
            show(pair.first, key in sw)
            pair.second.paint(sw[key] == true)
        }
        val ch = list(s.optJSONArray("choices")).associateBy { it.optString("key") }
        for ((key, pair) in choices) {
            val o = ch[key]
            show(pair.first, o != null)
            if (o != null) pair.second.text = choiceLabel(key, o.optInt("value"))
        }
        show(autoOff.first, s.has("autoOff"))
        autoOff.second.text = autoOffLabel(s)
        tidy(budsCard)

        val v = s.optJSONObject("volumeLimit")
        show(volumeLabel, v != null)
        show(volumeCard, v != null)
        if (v != null) {
            limit.second.paint(v.optBoolean("on"))
            limitDb.second.text = getString(R.string.sc_db, v.optInt("db"))
            limitRate.second.text = getString(rateLabel(v.optInt("rate")))
            show(limitDb.first, v.optBoolean("on"))
            tidy(volumeCard)
        }

        val fw = s.optJSONArray("firmware")
        firmware.second.text = when (fw?.length()) {
            2 -> getString(R.string.sc_firmware_lr, fw.getString(0), fw.getString(1))
            1 -> fw.getString(0)
            else -> ""
        }
        show(firmware.first, (fw?.length() ?: 0) > 0)
        serial.second.text = s.optString("serial")
        show(serial.first, s.optString("serial").isNotEmpty() && !s.isNull("serial"))
    }

    private fun rateLabel(v: Int) = when (v) { 1 -> R.string.sc_refresh_10s; 2 -> R.string.sc_refresh_1m; else -> R.string.sc_refresh_realtime }

    private fun autoOffLabel(s: JSONObject): String {
        val a = s.optJSONObject("autoOff") ?: return ""
        if (!a.optBoolean("on")) return getString(R.string.anc_seg_off)
        val m = a.optJSONArray("minutes") ?: return ""
        return getString(R.string.sc_minutes, m.optInt(a.optInt("index")))
    }

    private fun pickMode(key: String, title: Int) {
        val f = state?.let { mode(it, key) } ?: return
        val vals = f.optJSONArray("values") ?: return
        val list = (0 until vals.length()).map { vals.getInt(it) }
        choose(title, list.map { label(f, it) to (f.optInt("value") == it) }) { set(key, list[it]) }
    }

    private fun pickChoice(key: String, title: Int) {
        val o = state?.optJSONArray("choices")?.let { list(it) }?.firstOrNull { it.optString("key") == key } ?: return
        val vals = o.optJSONArray("values") ?: return
        val list = (0 until vals.length()).map { vals.getInt(it) }
        choose(title, list.map { choiceLabel(key, it) to (o.optInt("value") == it) }) { set(key, list[it]) }
    }

    /** The cycle is a set: the sheet stays open, and the buds need at least 2 modes. */
    private fun pickCycle() {
        val s = state ?: return
        val c = s.optJSONObject("cycle") ?: return
        val keys = listOf("anc" to R.string.sc_mode_anc, "transparency" to R.string.anc_seg_trans, "normal" to R.string.sc_mode_normal)
        val picked = keys.map { c.optBoolean(it.first) }.toMutableList()
        val sheet = BottomSheetDialog(this).title(getString(R.string.sc_cycle_title)).message(getString(R.string.sc_cycle_rule))
            .dismissOnSelect(false)
        fun items(): List<BottomSheetDialog.Item> = keys.mapIndexed { i, (_, label) ->
            BottomSheetDialog.Item(getString(label), picked[i]) {
                val next = picked.toMutableList().also { it[i] = !it[i] }
                if (next.count { it } < 2) return@Item
                picked[i] = next[i]
                set("cycle", JSONObject().apply { keys.forEachIndexed { j, k -> put(k.first, picked[j]) } })
                sheet.items(items())
            }
        }
        sheet.items(items()).confirm(getString(R.string.gesture_done)) { sheet.close() }.show()
    }

    private fun confirmReset() {
        val sheet = BottomSheetDialog(this)
        sheet.title(getString(R.string.sc_reset_buttons_title)).message(getString(R.string.sc_reset_buttons_body))
            .confirm(getString(R.string.sc_reset)) { manager?.soundcoreResetButtons(); sheet.close() }
            .cancel(getString(R.string.dialog_cancel)) { sheet.close() }
            .show()
    }

    private fun pickAutoOff() {
        val s = state ?: return
        val a = s.optJSONObject("autoOff") ?: return
        val m = a.optJSONArray("minutes") ?: return
        val on = a.optBoolean("on")
        val options = listOf(getString(R.string.anc_seg_off) to !on) +
            (0 until m.length()).map { getString(R.string.sc_minutes, m.getInt(it)) to (on && a.optInt("index") == it) }
        choose(R.string.sc_auto_off_title, options, getString(R.string.sc_auto_off_body)) { i ->
            set("autoOff", if (i == 0) JSONObject().put("on", false) else JSONObject().put("on", true).put("index", i - 1))
        }
    }

    private fun pickLimitDb() {
        val v = state?.optJSONObject("volumeLimit") ?: return
        val range = v.optJSONArray("range")
        val list = ((range?.optInt(0) ?: 75)..(range?.optInt(1) ?: 100) step 5).toList()
        choose(R.string.sc_volume_limit_db_title, list.map { getString(R.string.sc_db, it) to (v.optInt("db") == it) }) {
            set("volumeLimit", JSONObject().put("db", list[it]))
        }
    }

    private fun pickLimitRate() {
        val v = state?.optJSONObject("volumeLimit") ?: return
        choose(R.string.sc_refresh_title, listOf(0, 1, 2).map { getString(rateLabel(it)) to (v.optInt("rate") == it) },
            getString(R.string.sc_refresh_body)) { set("volumeLimit", JSONObject().put("rate", it)) }
    }
}
