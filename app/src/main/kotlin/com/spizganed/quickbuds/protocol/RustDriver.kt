package com.spizganed.quickbuds.protocol

import android.content.Context
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import com.spizganed.quickbuds.ui.ThemeRes
import org.json.JSONArray
import org.json.JSONObject

/**
 * Driver mode of the Rust core (docs/CORE-API.md): the OPPO driver in `core/` drives the link. The shell
 * ([com.spizganed.quickbuds.bluetooth.BudsConnectionManager]) owns the socket and the UI; this class feeds the
 * driver and runs its effects. All driver calls run on one thread, in order.
 *
 * Dev tools › Rust engine turns it on. Off (the default), the Kotlin layer drives the buds and [RustShadow] compares.
 */
class RustDriver(private val shell: Shell) {

    /** What the driver asks the shell to do. Every call comes on the driver thread. */
    interface Shell {
        fun write(frame: ByteArray)
        fun log(line: String)
        /** The state changed: [state] is the core's state JSON (`core-jni` `state_json`). */
        fun state(state: JSONObject)
        fun save(key: String, value: Any?)
        fun ready()
        fun findPhone(ring: Boolean)
        fun hearing(event: JSONObject)
    }

    private val thread = HandlerThread("rust-driver").apply { start() }
    private val handler = Handler(thread.looper)
    private val handle: Long = if (loaded) nativeNew() else 0L
    private val tickRun: Runnable = Runnable { drive { nativeTick(handle, now()) } }

    /** The native library loaded: driver mode can run. */
    val available: Boolean get() = handle != 0L

    private fun now() = SystemClock.elapsedRealtime()

    /** Runs one driver call on the driver thread, then its effects, then schedules the next tick. */
    private fun drive(call: () -> String?): Boolean = handler.post {
        if (handle == 0L) return@post
        val text = runCatching { call() }.getOrNull()
        if (text != null) effects(text)
        handler.removeCallbacks(tickRun)
        val next = nativeNextWake(handle)
        if (next >= 0) handler.postDelayed(tickRun, maxOf(0L, next - now()))
    }

    private fun effects(text: String) {
        val list = runCatching { JSONArray(text) }.getOrElse { shell.log("RUST: bad effects: ${it.message}"); return }
        var changed = false
        for (i in 0 until list.length()) {
            val e = list.getJSONObject(i)
            when {
                e.has("send") -> shell.write(unhex(e.getString("send")))
                e.has("log") -> {
                    val human = e.optString("human").takeIf { !e.isNull("human") }
                    val hex = e.optString("hex")
                    shell.log(listOfNotNull(e.getString("log") + ":", hex.takeIf { it.isNotEmpty() }?.let(::spaced), human?.let { "($it)" }).joinToString(" "))
                }
                e.has("state") -> changed = true
                e.has("save") -> shell.save(e.getString("save"), e.opt("value").takeIf { it != JSONObject.NULL })
                e.has("ready") -> shell.ready()
                e.has("findPhone") -> shell.findPhone(e.getBoolean("findPhone"))
                e.has("hearing") -> shell.hearing(e.getJSONObject("hearing"))
            }
        }
        if (changed) nativeState(handle)?.let { st -> runCatching { JSONObject(st) }.getOrNull()?.let(shell::state) }
    }

    /** The user's settings the driver reads; [connected] and a model pick pass them in. */
    fun settings(c: Context, eqJson: String?, caseLevel: Int?) {
        val prefs = c.getSharedPreferences(ThemeRes.PREFS_NAME, Context.MODE_PRIVATE)
        val clear = prefs.getBoolean(com.spizganed.quickbuds.bluetooth.BudsService.PREF_CLEAR_BATTERY, false)
        val product = Capabilities.productId(c)
        val manual = ModelCatalog.manual(c)
        val spatial = com.spizganed.quickbuds.bluetooth.PhoneSpatial.supported(c)
        handler.post { nativeSettings(handle, product, manual, clear, spatial, caseLevel ?: -1, eqJson) }
    }

    /** [soundcore]: the brand of this link ([com.spizganed.quickbuds.bluetooth.ConnectPolicy.brand]). */
    fun connected(name: String, soundcore: Boolean = false) = drive {
        nativeBrand(handle, soundcore)
        nativeConnected(handle, now(), name)
    }

    fun bytes(buffer: ByteArray, count: Int) {
        val data = buffer.copyOf(count)
        drive { nativeBytes(handle, now(), System.currentTimeMillis() / 1000, data) }
    }

    /** A UI command (`core-jni` `command`): `{"c": <name>, ...}`. */
    fun command(json: JSONObject) = drive {
        nativeCommand(handle, now(), json.toString()).also { if (it == null) shell.log("RUST: command refused: $json") }
    }

    fun command(name: String, vararg fields: Pair<String, Any?>) =
        command(JSONObject().put("c", name).apply { for ((k, v) in fields) put(k, v ?: JSONObject.NULL) })

    fun disconnected() = drive { nativeDisconnected(handle, now()) }

    fun caseStandIn(level: Int, charging: Boolean) = drive { nativeCaseStandIn(handle, now(), level, charging) }

    companion object {
        private val loaded: Boolean = try { System.loadLibrary("buds_core_jni"); true } catch (_: Throwable) { false }

        /** Dev tools › Rust engine (default off). The next connect reads it. */
        const val PREF = "devRustEngine"

        fun enabled(c: Context): Boolean =
            loaded && c.getSharedPreferences(ThemeRes.PREFS_NAME, Context.MODE_PRIVATE).getBoolean(PREF, false)

        /** The native library loaded: Soundcore buds, which only the core drives, can connect. */
        val libraryLoaded: Boolean get() = loaded

        fun unhex(t: String): ByteArray = ByteArray(t.length / 2) { t.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

        fun hex(b: ByteArray): String = b.joinToString("") { "%02X".format(it.toInt() and 0xFF) }

        private fun spaced(hex: String) = hex.chunked(2).joinToString(" ")

        @JvmStatic private external fun nativeNew(): Long
        @JvmStatic private external fun nativeBrand(h: Long, soundcore: Boolean)
        @JvmStatic private external fun nativeSettings(h: Long, product: String?, manual: String?, clearBattery: Boolean,
            phoneSpatial: Boolean, caseLevel: Int, eq: String?)
        @JvmStatic private external fun nativeConnected(h: Long, now: Long, name: String): String?
        @JvmStatic private external fun nativeBytes(h: Long, now: Long, unix: Long, data: ByteArray): String?
        @JvmStatic private external fun nativeCommand(h: Long, now: Long, json: String): String?
        @JvmStatic private external fun nativeTick(h: Long, now: Long): String?
        @JvmStatic private external fun nativeDisconnected(h: Long, now: Long): String?
        @JvmStatic private external fun nativeCaseStandIn(h: Long, now: Long, level: Int, charging: Boolean): String?
        @JvmStatic private external fun nativeNextWake(h: Long): Long
        @JvmStatic private external fun nativeState(h: Long): String?
    }
}
