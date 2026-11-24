package com.spizganed.quickbuds.ui

import android.content.Intent
import android.view.View
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView
import com.spizganed.quickbuds.R
import org.json.JSONObject

/**
 * Earbud settings of Soundcore buds: the noise control details, the gestures, the switches, auto power off, the
 * volume limit and the firmware. A row shows only when the model has the setting (its key is in the state).
 * Keys and values: docs/SOUNDCORE-PROTOCOL.md.
 */
class SoundcoreSettingsActivity : SoundcoreScreen() {

    private lateinit var transparency: Pair<LinearLayout, TextView>
    private lateinit var ancMode: Pair<LinearLayout, TextView>
    private lateinit var ancLevel: Pair<LinearLayout, TextView>
    private lateinit var transport: Pair<LinearLayout, TextView>
    private lateinit var adaptive: Pair<LinearLayout, TextView>
    private lateinit var wind: Pair<LinearLayout, Switch>
    private lateinit var cycle: Pair<LinearLayout, TextView>
    private val switches = mutableMapOf<String, Pair<LinearLayout, Switch>>()
    private lateinit var autoOff: Pair<LinearLayout, TextView>
    private lateinit var limit: Pair<LinearLayout, Switch>
    private lateinit var limitDb: Pair<LinearLayout, TextView>
    private lateinit var limitRate: Pair<LinearLayout, TextView>
    private lateinit var firmware: Pair<LinearLayout, TextView>
    private lateinit var serial: Pair<LinearLayout, TextView>
    private lateinit var volumeLabel: View
    private lateinit var volumeCard: View

    override fun build() {
        root.addView(SettingRowFactory.title(this, R.string.row_earbuds_title))

        section(R.string.anc_section)
        val noise = card()
        transparency = choiceRow(noise, R.string.sc_transparency_title) { pickTransparency() }
        ancMode = choiceRow(noise, R.string.sc_anc_mode_title) { pickAncMode() }
        ancLevel = choiceRow(noise, R.string.sc_anc_level_title) { pickAncLevel() }
        transport = choiceRow(noise, R.string.sc_transport_title) { pickTransport() }
        adaptive = SettingRowFactory.build(this, 0, R.string.sc_adaptive_title, 0, null).let { row ->
            SettingRowFactory.addRow(noise, row)
            row to SettingRowFactory.subtitle(this, row)
        }
        wind = switchRow(noise, R.string.row_wind_noise_title, R.string.sc_wind_sub, "windNoise")

        section(R.string.sc_section_controls)
        val controls = card()
        SettingRowFactory.addRow(controls, SettingRowFactory.build(this, 0, R.string.gesture_title, R.string.sc_buttons_sub,
            SettingRowFactory.buildChevron(this)) { startActivity(Intent(this, SoundcoreButtonsActivity::class.java)) })
        cycle = choiceRow(controls, R.string.sc_cycle_title) { pickCycle() }
        SettingRowFactory.addRow(controls, SettingRowFactory.build(this, 0, R.string.sc_reset_buttons_title, R.string.sc_reset_buttons_sub,
            null) { confirmReset() })

        section(R.string.sc_section_earbuds)
        val buds = card()
        switches["touchTone"] = switchRow(buds, R.string.sc_touch_tone_title, R.string.sc_touch_tone_sub, "touchTone")
        switches["wearingTone"] = switchRow(buds, R.string.sc_wearing_tone_title, R.string.sc_wearing_tone_sub, "wearingTone")
        switches["lowBattery"] = switchRow(buds, R.string.sc_low_battery_title, R.string.sc_low_battery_sub, "lowBattery")
        switches["wearDetection"] = switchRow(buds, R.string.row_wear_title, R.string.row_wear_sub, "wearDetection")
        switches["leak"] = switchRow(buds, R.string.sc_leak_title, R.string.sc_leak_sub, "leak")
        autoOff = choiceRow(buds, R.string.sc_auto_off_title) { pickAutoOff() }

        volumeLabel = section(R.string.sc_section_volume)
        val volume = card().also { volumeCard = it }
        limit = SettingRowFactory.buildSwitch(this, false).let { sw ->
            sw.setOnCheckedChangeListener { _, on -> if (!painting) set("volumeLimit", JSONObject().put("on", on)) }
            val row = SettingRowFactory.build(this, 0, R.string.sc_volume_limit_title, R.string.sc_volume_limit_sub, sw) { sw.performClick() }
            SettingRowFactory.addRow(volume, row)
            row to sw
        }
        limitDb = choiceRow(volume, R.string.sc_volume_limit_db_title) { pickLimitDb() }
        limitRate = choiceRow(volume, R.string.sc_refresh_title) { pickLimitRate() }

        section(R.string.sc_section_about)
        val about = card()
        firmware = SettingRowFactory.build(this, 0, R.string.row_firmware_title, 0, null).let { row ->
            SettingRowFactory.addRow(about, row); row to SettingRowFactory.subtitle(this, row)
        }
        serial = SettingRowFactory.build(this, 0, R.string.sc_serial_title, 0, null).let { row ->
            SettingRowFactory.addRow(about, row); row to SettingRowFactory.subtitle(this, row)
        }
    }

    /** Shows or hides a row, and the divider that [SettingRowFactory.addRow] put before it. A card's first row never hides. */
    private fun show(row: View, on: Boolean) {
        val v = if (on) View.VISIBLE else View.GONE
        row.visibility = v
        val parent = row.parent as? android.view.ViewGroup ?: return
        val i = parent.indexOfChild(row)
        if (i > 0) parent.getChildAt(i - 1).takeIf { it !is LinearLayout }?.visibility = v
    }

    override fun render(s: JSONObject) {
        transparency.second.text = getString(transparencyLabel(s.optInt("transparencyMode")))
        ancMode.second.text = getString(ancModeLabel(s.optInt("ancMode")))
        ancLevel.second.text = levelLabel(s, s.optInt("ancLevel"))
        // A sub-setting shows only when its mode applies, so a write never needs a mode change first.
        show(ancLevel.first, s.optInt("ancMode") == 0)
        show(transport.first, s.has("transport") && s.optInt("ancMode") == 2)
        transport.second.text = getString(if (s.optInt("transport") == 3) R.string.sc_transport_car else R.string.sc_transport_plane)
        show(adaptive.first, s.has("adaptiveLevel") && s.optInt("ancMode") == 1)
        adaptive.second.text = getString(adaptiveLabel(s.optInt("adaptiveLevel")))
        wind.second.paint(s.optBoolean("windNoise"))
        SettingRowFactory.subtitle(this, wind.first).setText(if (s.optBoolean("windDetected")) R.string.sc_wind_detected else R.string.sc_wind_sub)

        val c = s.optJSONObject("cycle")
        cycle.second.text = listOfNotNull(
            getString(R.string.sc_mode_anc).takeIf { c?.optBoolean("anc") == true },
            getString(R.string.anc_seg_trans).takeIf { c?.optBoolean("transparency") == true },
            getString(R.string.sc_mode_normal).takeIf { c?.optBoolean("normal") == true },
        ).joinToString(" · ")

        for ((key, pair) in switches) {
            show(pair.first, s.has(key))
            pair.second.paint(s.optBoolean(key))
        }
        autoOff.second.text = autoOffLabel(s)

        val v = s.optJSONObject("volumeLimit")
        show(volumeLabel, v != null)
        show(volumeCard, v != null)
        if (v != null) {
            limit.second.paint(v.optBoolean("on"))
            limitDb.second.text = getString(R.string.sc_db, v.optInt("db"))
            limitRate.second.text = getString(rateLabel(v.optInt("rate")))
            show(limitDb.first, v.optBoolean("on"))
        }

        val fw = s.optJSONArray("firmware")
        firmware.second.text = when (fw?.length()) {
            2 -> getString(R.string.sc_firmware_lr, fw.getString(0), fw.getString(1))
            1 -> fw.getString(0)
            else -> ""
        }
        serial.second.text = s.optString("serial")
    }

    private fun transparencyLabel(v: Int) = if (v == 1) R.string.sc_transparency_vocal else R.string.sc_transparency_full
    private fun ancModeLabel(v: Int) = when (v) { 1 -> R.string.sc_anc_adaptive; 2 -> R.string.sc_anc_transport; else -> R.string.sc_anc_manual }
    private fun adaptiveLabel(v: Int) = when (v) { 1 -> R.string.sc_noise_medium; 2 -> R.string.sc_noise_high; else -> R.string.sc_noise_low }
    private fun rateLabel(v: Int) = when (v) { 1 -> R.string.sc_refresh_10s; 2 -> R.string.sc_refresh_1m; else -> R.string.sc_refresh_realtime }

    /** The A3936 names its 3 strengths; the A3957 has 5 numbered ones. */
    private fun levelLabel(s: JSONObject, v: Int): String = if (s.optInt("ancLevels") == 3)
        getString(when (v) { 1 -> R.string.sc_level_weak; 2 -> R.string.sc_level_moderate; else -> R.string.sc_level_strong })
        else getString(R.string.sc_level_n, v)

    private fun autoOffLabel(s: JSONObject): String {
        val a = s.optJSONObject("autoOff") ?: return ""
        if (!a.optBoolean("on")) return getString(R.string.anc_seg_off)
        val m = a.optJSONArray("minutes") ?: return ""
        return getString(R.string.sc_minutes, m.optInt(a.optInt("index")))
    }

    private fun pickTransparency() {
        val s = state ?: return
        choose(R.string.sc_transparency_title, listOf(0, 1).map { getString(transparencyLabel(it)) to (s.optInt("transparencyMode") == it) }) {
            set("transparencyMode", it)
        }
    }

    private fun pickAncMode() {
        val s = state ?: return
        val modes = s.optJSONArray("ancModes") ?: return
        val list = (0 until modes.length()).map { modes.getInt(it) }
        choose(R.string.sc_anc_mode_title, list.map { getString(ancModeLabel(it)) to (s.optInt("ancMode") == it) }) { set("ancMode", list[it]) }
    }

    private fun pickAncLevel() {
        val s = state ?: return
        val levels = (1..s.optInt("ancLevels", 3)).toList()
        choose(R.string.sc_anc_level_title, levels.map { levelLabel(s, it) to (s.optInt("ancLevel") == it) }) { set("ancLevel", levels[it]) }
    }

    private fun pickTransport() {
        val s = state ?: return
        val list = listOf(0, 3)
        choose(R.string.sc_transport_title, list.map {
            getString(if (it == 3) R.string.sc_transport_car else R.string.sc_transport_plane) to (s.optInt("transport") == it)
        }) { set("transport", list[it]) }
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
