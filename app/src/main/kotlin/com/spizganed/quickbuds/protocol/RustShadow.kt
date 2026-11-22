package com.spizganed.quickbuds.protocol

import android.content.Context
import android.os.SystemClock
import com.spizganed.quickbuds.bluetooth.BudsService
import com.spizganed.quickbuds.bluetooth.PhoneSpatial
import com.spizganed.quickbuds.ui.Rows
import com.spizganed.quickbuds.ui.ThemeRes
import com.spizganed.quickbuds.widget.WidgetStateStore
import org.json.JSONObject

/**
 * Shadow mode of the Rust core (docs/CORE-API.md, docs/ARCHITECTURE.md): the OPPO driver in `core/` gets the same
 * bytes as this Kotlin layer, and its frames go nowhere. [check] compares the two shared states and logs each new
 * difference as one `RUST DIFF:` line. Users see no change. Without the native library, every call does nothing.
 */
object RustShadow {
    private val loaded: Boolean = try { System.loadLibrary("buds_core_jni"); true } catch (_: Throwable) { false }
    private var handle = 0L
    /** The last logged difference, so the log gets a line only when it changes. */
    private var lastDiff: String? = null

    @Synchronized
    fun connected(c: Context, name: String) {
        if (!loaded) return
        if (handle == 0L) handle = nativeNew()
        val clear = c.getSharedPreferences(ThemeRes.PREFS_NAME, Context.MODE_PRIVATE).getBoolean(BudsService.PREF_CLEAR_BATTERY, false)
        nativeConnected(handle, SystemClock.elapsedRealtime(), name, Capabilities.productId(c), ModelCatalog.manual(c),
            clear, PhoneSpatial.supported(c))
        lastDiff = null
    }

    @Synchronized
    fun bytes(buffer: ByteArray, count: Int) {
        if (handle == 0L) return
        nativeBytes(handle, SystemClock.elapsedRealtime(), System.currentTimeMillis() / 1000, buffer.copyOf(count))
    }

    @Synchronized
    fun disconnected() {
        if (handle != 0L) nativeDisconnected(handle, SystemClock.elapsedRealtime())
    }

    /** Compares the states and logs a new difference, or the end of one. */
    @Synchronized
    fun check(c: Context, log: (String) -> Unit) {
        if (handle == 0L) return
        val rust = nativeCommon(handle, SystemClock.elapsedRealtime())?.let { JSONObject(it) }
        val diff = if (rust == null) "core failed" else diff(WidgetStateStore.read(c), Rows.of(c), rust).joinToString("; ")
        if (diff == (lastDiff ?: "")) return
        lastDiff = diff
        log(if (diff.isEmpty()) "RUST DIFF: none" else "RUST DIFF: $diff")
    }

    /** A raw wear status as the core names it (PROTOCOL.md section 13). */
    private fun wear(raw: Int) = when (raw) {
        3, 7 -> "ear"
        0, 4 -> "case"
        1, 5 -> "out"
        else -> "?"
    }

    /** Each field where the core's state differs from Kotlin's, as `field kotlin=… rust=…`. */
    fun diff(k: WidgetStateStore.State, rows: Set<String>, rust: JSONObject): List<String> {
        val out = mutableListOf<String>()
        fun same(field: String, kotlin: Any?, core: Any?) { if (kotlin != core) out += "$field kotlin=$kotlin rust=$core" }
        if (!k.connected || !rust.optBoolean("connected")) {
            same("connected", k.connected, rust.optBoolean("connected"))
            return out
        }
        val battery = rust.optJSONArray("battery")
        listOf("left" to k.leftBattery, "right" to k.rightBattery, "case" to k.caseBattery).forEachIndexed { i, (name, level) ->
            same("battery.$name", level, battery?.optInt(i, -1) ?: -1)
        }
        val wear = rust.optJSONArray("wear")
        same("wear.left", wear(k.leftStatus), wear?.optString(0))
        same("wear.right", wear(k.rightStatus), wear?.optString(1))
        same("noise", k.ancMode, if (rust.isNull("noise")) null else rust.optString("noise"))
        same("lowLatency", k.gameMode, if (rust.isNull("lowLatency")) false else rust.optBoolean("lowLatency"))
        val features = rust.optJSONArray("features")?.let { a -> (0 until a.length()).map { a.getString(it) }.toSet() } ?: emptySet()
        (rows - features).takeIf { it.isNotEmpty() }?.let { out += "features only kotlin=${it.sorted()}" }
        (features - rows).takeIf { it.isNotEmpty() }?.let { out += "features only rust=${it.sorted()}" }
        return out
    }

    private external fun nativeNew(): Long
    private external fun nativeConnected(h: Long, now: Long, name: String, product: String?, manual: String?, clearBattery: Boolean, phoneSpatial: Boolean)
    private external fun nativeBytes(h: Long, now: Long, unix: Long, data: ByteArray)
    private external fun nativeDisconnected(h: Long, now: Long)
    private external fun nativeCommon(h: Long, now: Long): String?
}
