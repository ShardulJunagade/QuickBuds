package com.spizganed.quickbuds.ui

import android.view.Gravity
import android.widget.LinearLayout
import android.widget.SeekBar
import android.widget.TextView
import com.spizganed.quickbuds.R
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

/**
 * The equalizer of Soundcore buds: the model's presets and, on most models, a custom curve of 8 bands. A slider writes the
 * custom curve when it is let go; the first change starts from the bands of the preset in use. The state lists
 * the presets, the band frequencies and the range (`eqPresets`, `eqHz`, `eqRange`).
 */
class SoundcoreEqActivity : SoundcoreScreen() {

    private lateinit var presetCard: LinearLayout
    private val presetRows = mutableMapOf<Int, LinearLayout>()
    private lateinit var customCard: LinearLayout
    private lateinit var customLabel: android.view.View
    private val sliders = mutableListOf<Pair<SeekBar, TextView>>()
    /** A slider is held: the state does not move it. */
    private var dragging = false

    override fun build() {
        root.addView(SettingRowFactory.title(this, R.string.row_eq_title))
        customLabel = section(R.string.sc_eq_custom)
        customCard = card()
        section(R.string.sc_eq_presets)
        presetCard = card()
    }

    override fun render(s: JSONObject) {
        val presets = s.optJSONArray("eqPresets") ?: return
        if (presetRows.isEmpty()) buildPresets(presets)
        if (sliders.isEmpty()) buildSliders(s)
        // Some models take only the presets (`eqCustom` false).
        val custom = s.optBoolean("eqCustom", true)
        customLabel.visibility = if (custom) android.view.View.VISIBLE else android.view.View.GONE
        customCard.visibility = customLabel.visibility
        val current = s.optInt("eqPreset")
        for ((id, row) in presetRows) {
            row.findViewWithTag<TextView>(SettingRowFactory.TITLE_TAG)?.setTextColor(ThemeRes.color(this,
                if (id == current) R.attr.appColorAccent else R.attr.appColorTextPrimary))
            row.findViewWithTag<android.view.View>(CHECK)?.visibility = if (id == current) android.view.View.VISIBLE else android.view.View.INVISIBLE
        }
        if (!dragging) bands(s).forEachIndexed { i, v -> sliders.getOrNull(i)?.let { (bar, text) -> bar.progress = toProgress(v); text.text = db(v) } }
    }

    private fun buildPresets(presets: JSONArray) {
        for (i in 0 until presets.length()) {
            val p = presets.getJSONObject(i)
            val id = p.getInt("id")
            val check = android.widget.ImageView(this).apply {
                setImageDrawable(ThemeRes.tint(this@SoundcoreEqActivity, R.drawable.ic_check, ThemeRes.color(this@SoundcoreEqActivity, R.attr.appColorAccent)))
                tag = CHECK
                visibility = android.view.View.INVISIBLE
            }
            val row = SettingRowFactory.build(this, 0, presetName(p.getString("name")), 0, check) {
                set("eq", JSONObject().put("preset", id))
            }
            SettingRowFactory.addRow(presetCard, row)
            presetRows[id] = row
        }
    }

    private fun buildSliders(s: JSONObject) {
        val hz = s.optJSONArray("eqHz") ?: return
        val dp = { v: Float -> ThemeRes.dp(this, v) }
        for (i in 0 until hz.length()) {
            val value = TextView(this).apply {
                setTextColor(ThemeRes.color(this@SoundcoreEqActivity, R.attr.appColorTextSecondary))
                textSize = 13f
                gravity = Gravity.END
                minWidth = dp(64f)
            }
            val accent = android.content.res.ColorStateList.valueOf(ThemeRes.color(this, R.attr.appColorAccent))
            val track = android.content.res.ColorStateList.valueOf(ThemeRes.color(this, R.attr.appColorTextSecondary))
            val bar = SeekBar(this).apply {
                max = STEPS
                // The theme's accent, as the app's other controls; Android's default is red.
                progressTintList = accent
                thumbTintList = accent
                progressBackgroundTintList = track
                setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                    override fun onProgressChanged(b: SeekBar, p: Int, fromUser: Boolean) { if (fromUser) value.text = db(fromProgress(p)) }
                    override fun onStartTrackingTouch(b: SeekBar) { dragging = true }
                    override fun onStopTrackingTouch(b: SeekBar) {
                        dragging = false
                        if (!painting) writeCustom()
                    }
                })
            }
            val label = TextView(this).apply {
                text = freq(hz.getInt(i))
                setTextColor(ThemeRes.color(this@SoundcoreEqActivity, R.attr.appColorTextPrimary))
                textSize = 15f
                minWidth = dp(72f)
            }
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                setPadding(dp(16f), dp(6f), dp(16f), dp(6f))
                addView(label)
                addView(bar, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f))
                addView(value)
            }
            customCard.addView(row)
            sliders += bar to value
        }
    }

    /** The bands in use: the custom curve, or the preset's own. */
    private fun bands(s: JSONObject): List<Int> {
        val id = s.optInt("eqPreset")
        val presets = s.optJSONArray("eqPresets")
        val preset = presets?.let { a -> (0 until a.length()).map { a.getJSONObject(it) }.firstOrNull { it.getInt("id") == id } }
        val a = preset?.optJSONArray("bands") ?: s.optJSONArray("eqBands") ?: return emptyList()
        return (0 until a.length()).map { a.getInt(it) }
    }

    private fun writeCustom() {
        val list = JSONArray(sliders.map { fromProgress(it.first.progress) })
        set("eq", JSONObject().put("preset", CUSTOM).put("bands", list))
    }

    private fun freq(hz: Int): String = if (hz >= 1000) getString(R.string.sc_khz, String.format(Locale.getDefault(), "%.1f", hz / 1000.0).removeSuffix(".0").removeSuffix(",0"))
        else getString(R.string.sc_hz, hz)

    private fun db(tenths: Int): String = getString(R.string.sc_db_value, String.format(Locale.getDefault(), "%+.1f", tenths / 10.0))

    private fun presetName(key: String): Int = PRESET_NAMES[key] ?: R.string.sc_eq_custom

    private companion object {
        const val CHECK = "scEqCheck"
        const val CUSTOM = 0xFEFE
        /** The sliders: -12 dB to +12 dB in 0.5 dB steps, in tenths of a dB. */
        const val MIN = -120
        const val STEP = 5
        const val STEPS = 48
        fun toProgress(tenths: Int) = ((tenths.coerceIn(MIN, -MIN) - MIN) / STEP)
        fun fromProgress(p: Int) = MIN + p * STEP

        /** The preset names of `soundcore::presets`. */
        val PRESET_NAMES = mapOf(
            "SoundcoreSignature" to R.string.sc_eq_signature, "Acoustic" to R.string.sc_eq_acoustic,
            "BassBooster" to R.string.sc_eq_bass_booster, "BassReducer" to R.string.sc_eq_bass_reducer,
            "Classical" to R.string.sc_eq_classical, "Podcast" to R.string.sc_eq_podcast, "Dance" to R.string.sc_eq_dance,
            "Deep" to R.string.sc_eq_deep, "Electronic" to R.string.sc_eq_electronic, "Flat" to R.string.sc_eq_flat,
            "HipHop" to R.string.sc_eq_hip_hop, "Jazz" to R.string.sc_eq_jazz, "Latin" to R.string.sc_eq_latin,
            "Lounge" to R.string.sc_eq_lounge, "Piano" to R.string.sc_eq_piano, "Pop" to R.string.sc_eq_pop,
            "RnB" to R.string.sc_eq_rnb, "Rock" to R.string.sc_eq_rock, "SmallSpeakers" to R.string.sc_eq_small_speakers,
            "SpokenWord" to R.string.sc_eq_spoken_word, "TrebleBooster" to R.string.sc_eq_treble_booster,
            "TrebleReducer" to R.string.sc_eq_treble_reducer, "Balanced" to R.string.sc_eq_balanced,
            "VolumeBooster" to R.string.sc_eq_volume_booster,
        )
    }
}
