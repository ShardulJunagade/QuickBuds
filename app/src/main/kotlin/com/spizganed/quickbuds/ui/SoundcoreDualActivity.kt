package com.spizganed.quickbuds.ui

import android.widget.LinearLayout
import android.widget.Switch
import com.spizganed.quickbuds.R
import org.json.JSONObject

/**
 * Dual connection of Soundcore buds: the switch and the devices the buds know (`0b 01`). A tap on a device
 * connects or disconnects it, or forgets it.
 */
class SoundcoreDualActivity : SoundcoreScreen() {

    private lateinit var dual: Switch
    private lateinit var label: android.view.View
    private lateinit var devices: LinearLayout
    private var shown: String? = null

    override fun build() {
        root.addView(SettingRowFactory.title(this, R.string.dual_title))
        val top = card()
        dual = switchRow(top, R.string.row_dual_title, R.string.dual_switch_sub, "dual").second
        label = section(R.string.dual_section_all)
        devices = card()
    }

    override fun render(s: JSONObject) {
        dual.paint(s.optBoolean("dual"))
        val list = s.optJSONArray("devices")
        val on = s.optBoolean("dual") && list != null && list.length() > 0
        label.visibility = if (on) android.view.View.VISIBLE else android.view.View.GONE
        devices.visibility = label.visibility
        val text = list?.toString()
        if (text == shown) return
        shown = text
        devices.removeAllViews()
        for (i in 0 until (list?.length() ?: 0)) {
            val d = list!!.getJSONObject(i)
            val row = SettingRowFactory.build(this, R.drawable.ic_devices, 0, 0, null) { act(d) }
            row.findViewWithTag<android.widget.TextView>(SettingRowFactory.TITLE_TAG)?.text = d.optString("name").ifEmpty { d.optString("mac") }
            SettingRowFactory.subtitle(this, row).setText(if (d.optBoolean("connected")) R.string.dual_connected else R.string.dual_not_connected)
            SettingRowFactory.addRow(devices, row)
        }
    }

    private fun act(d: JSONObject) {
        val mac = d.optString("mac")
        val connected = d.optBoolean("connected")
        val name = d.optString("name").ifEmpty { mac }
        val sheet = BottomSheetDialog(this)
        sheet.title(name).items(listOf(
            BottomSheetDialog.Item(getString(if (connected) R.string.conn_action_disconnect else R.string.conn_action_connect), false) {
                if (connected) confirm(getString(R.string.dual_disconnect_q, name), R.string.conn_action_disconnect) { manager?.soundcoreDevice("disconnect", mac) }
                else manager?.soundcoreDevice("connect", mac)
            },
            BottomSheetDialog.Item(getString(R.string.sc_device_forget), false) {
                confirm(getString(R.string.sc_forget_q, name), R.string.sc_device_forget) { manager?.soundcoreDevice("forget", mac) }
            },
        )).show()
    }

    /** Disconnect and forget can cut this phone's own link: they ask first. */
    private fun confirm(question: String, label: Int, action: () -> Unit) {
        val sheet = BottomSheetDialog(this)
        sheet.title(question)
            .confirm(getString(label)) { action(); sheet.close() }
            .cancel(getString(R.string.dialog_cancel)) { sheet.close() }
            .show()
    }
}
