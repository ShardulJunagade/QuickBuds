package com.spizganed.quickbuds.ui

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Switch
import android.widget.TextView
import com.spizganed.quickbuds.bluetooth.BudsConnectionManager
import com.spizganed.quickbuds.bluetooth.BudsService
import com.spizganed.quickbuds.protocol.Soundcore
import org.json.JSONObject

/**
 * The base of the Soundcore screens: binds to the service, keeps the last Soundcore state ([Soundcore]) and
 * calls [render] when it changes. A screen builds its rows once in [build] and repaints them in [render]. Every
 * write goes to [BudsConnectionManager.soundcoreSet]; the core changes its copy of the state at once and sends
 * the new state back, so the rows repaint from the buds' values.
 */
abstract class SoundcoreScreen : Activity(), BudsConnectionManager.Listener {

    protected var manager: BudsConnectionManager? = null
    private var bound = false
    /** True while [render] runs, so a switch moved by code sends no write. */
    protected var painting = false
    protected lateinit var root: LinearLayout

    /** The last state: the live one, or the saved one while the service binds. */
    protected val state: JSONObject? get() = manager?.soundcore ?: Soundcore.state(this)

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            manager = (service as BudsService.LocalBinder).getService().manager
            manager?.addListener(this@SoundcoreScreen)
            paint()
        }

        override fun onServiceDisconnected(name: ComponentName?) { manager = null }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        // Before super.onCreate: see ThemeRes.select.
        ThemeRes.select(this)
        super.onCreate(savedInstanceState)
        root = SettingRowFactory.screen(this)
        build()
        setContentView(ScrollView(this).apply { addView(root) })
        paint()
    }

    override fun onStart() {
        super.onStart()
        bound = bindService(Intent(this, BudsService::class.java), connection, Context.BIND_AUTO_CREATE)
    }

    override fun onStop() {
        manager?.removeListener(this)
        if (bound) unbindService(connection)
        bound = false
        manager = null
        super.onStop()
    }

    override fun onConnected(connected: Boolean) { paint() }
    override fun onBattery(left: Int?, case: Int?, right: Int?, chargingLeft: Boolean, chargingCase: Boolean, chargingRight: Boolean) {}
    override fun onSoundcore(state: JSONObject) { paint() }

    private fun paint() {
        val s = state ?: return
        painting = true
        try { render(s) } finally { painting = false }
    }

    protected abstract fun build()
    protected abstract fun render(s: JSONObject)

    /** One setting write. */
    protected fun set(key: String, value: Any) { manager?.soundcoreSet(key, value) }

    protected fun card(): LinearLayout = SettingRowFactory.card(this).also { root.addView(it) }

    protected fun section(textRes: Int): TextView = SettingRowFactory.sectionLabel(this, textRes).also { root.addView(it) }

    /** A switch row for one boolean setting [key]. Returns the switch for [render]. */
    protected fun switchRow(card: LinearLayout, titleRes: Int, subRes: Int, key: String): Pair<LinearLayout, Switch> {
        val sw = SettingRowFactory.buildSwitch(this, false)
        sw.setOnCheckedChangeListener { _, on -> if (!painting) set(key, on) }
        val row = SettingRowFactory.build(this, 0, titleRes, subRes, sw) { sw.performClick() }
        SettingRowFactory.addRow(card, row)
        return row to sw
    }

    /** A row that opens a chooser; its value text shows the current choice. */
    protected fun choiceRow(card: LinearLayout, titleRes: Int, onClick: () -> Unit): Pair<LinearLayout, TextView> {
        val row = SettingRowFactory.build(this, 0, titleRes, 0, SettingRowFactory.buildChevron(this), onClick = onClick)
        SettingRowFactory.addRow(card, row)
        return row to SettingRowFactory.subtitle(this, row)
    }

    /** A chooser sheet: one tap picks. */
    protected fun choose(titleRes: Int, options: List<Pair<String, Boolean>>, message: String? = null, pick: (Int) -> Unit) {
        BottomSheetDialog(this).title(getString(titleRes)).apply { if (message != null) message(message) }
            .items(options.mapIndexed { i, (label, selected) -> BottomSheetDialog.Item(label, selected) { pick(i) } })
            .show()
    }

    protected fun Switch.paint(on: Boolean) { if (isChecked != on) isChecked = on }
}
