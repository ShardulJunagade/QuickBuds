package com.spizganed.quickbuds.protocol

import android.content.Context
import android.os.SystemClock
import com.spizganed.quickbuds.bluetooth.BudsConnectionManager
import com.spizganed.quickbuds.bluetooth.BudsService
import com.spizganed.quickbuds.bluetooth.PhoneSpatial
import com.spizganed.quickbuds.ui.Rows
import com.spizganed.quickbuds.ui.ThemeRes
import com.spizganed.quickbuds.widget.WidgetStateStore
import org.json.JSONArray
import org.json.JSONObject

/**
 * Shadow mode of the Rust core (docs/CORE-API.md, docs/ARCHITECTURE.md): the OPPO driver in `core/` gets the same
 * bytes as this Kotlin layer, and its frames go nowhere. [check] compares the two states and logs each new
 * difference as one `RUST DIFF:` line. [checkInit] compares the commands of the init sequence once per link
 * (`RUST TX DIFF:`). Users see no change. Without the native library, every call does nothing.
 */
object RustShadow {
    private val loaded: Boolean = try { System.loadLibrary("buds_core_jni"); true } catch (_: Throwable) { false }
    private var handle = 0L
    /** The last logged difference, so the log gets a line only when it changes. */
    private var lastDiff: String? = null
    /** The commands this layer sent since the link came up, until [checkInit]. */
    private val initSent = mutableListOf<Int>()
    private var recordInit = false

    /** What the Kotlin layer reported through its listener and keeps nowhere else. Reset on each link. */
    class Seen {
        var tap: List<Int>? = null
        var preferred: String? = null
        var preferredRead = false
        var firmware: String? = null
        var fit: List<Int>? = null
        var pnc: List<Int>? = null
        val busy = sortedSetOf<Int>()
    }

    @Volatile private var seen = Seen()

    /** Records what the Kotlin layer reports. [BudsConnectionManager] registers it. */
    val recorder = object : BudsConnectionManager.Listener {
        override fun onConnected(connected: Boolean) {}
        override fun onBattery(left: Int?, case: Int?, right: Int?, chargingLeft: Boolean, chargingCase: Boolean, chargingRight: Boolean) {}
        override fun onTapLevel(level: Int, default: Int) { seen.tap = listOf(level, default) }
        override fun onPreferred(mac: String?) { seen.preferred = mac; seen.preferredRead = true }
        override fun onFirmware(version: String) { seen.firmware = version }
        override fun onFitResult(left: Int, right: Int) { seen.fit = listOf(left, right) }
        override fun onPersonalNoiseStored(stored: Boolean) { seen.pnc = listOf(1, if (stored) 1 else 0) }
        override fun onPersonalNoiseAck(status: Int) { seen.pnc = listOf(2, status) }
        override fun onPersonalNoiseResult(result: Int) { seen.pnc = listOf(3, result) }
        override fun onSettingBusy(key: Int, on: Boolean) { synchronized(seen.busy) { if (on) seen.busy += key else seen.busy -= key } }
    }

    @Synchronized
    fun connected(c: Context, name: String) {
        if (!loaded) return
        if (handle == 0L) handle = nativeNew()
        val clear = c.getSharedPreferences(ThemeRes.PREFS_NAME, Context.MODE_PRIVATE).getBoolean(BudsService.PREF_CLEAR_BATTERY, false)
        // The engine's busy list goes on across a reconnect, as in the core.
        val busy = synchronized(seen.busy) { seen.busy.toList() }
        seen = Seen().also { it.busy += busy }
        initSent.clear()
        recordInit = true
        nativeConnected(handle, SystemClock.elapsedRealtime(), name, Capabilities.productId(c), ModelCatalog.manual(c),
            clear, PhoneSpatial.supported(c))
        lastDiff = null
    }

    @Synchronized
    fun bytes(buffer: ByteArray, count: Int) {
        if (handle == 0L) return
        nativeBytes(handle, SystemClock.elapsedRealtime(), System.currentTimeMillis() / 1000, buffer.copyOf(count))
    }

    /** A frame this layer wrote to the buds: kept for [checkInit]. */
    @Synchronized
    fun sent(frame: ByteArray) {
        if (handle == 0L || !recordInit || frame.size < 6) return
        initSent += OpoProtocol.u16(frame, 4)
    }

    @Synchronized
    fun disconnected() {
        recordInit = false
        if (handle != 0L) nativeDisconnected(handle, SystemClock.elapsedRealtime())
    }

    /** Compares the states and logs a new difference, or the end of one. */
    @Synchronized
    fun check(c: Context, m: BudsConnectionManager, log: (String) -> Unit) {
        if (handle == 0L) return
        val rust = nativeCommon(handle, SystemClock.elapsedRealtime())?.let { JSONObject(it) }
        val diff = if (rust == null) "core failed"
            else (diff(WidgetStateStore.read(c), Rows.of(c), rust, m.eq, Capabilities.reply(c)) + more(m, rust)).joinToString("; ")
        if (diff == (lastDiff ?: "")) return
        lastDiff = diff
        log(if (diff.isEmpty()) "RUST DIFF: none" else "RUST DIFF: $diff")
    }

    /**
     * The commands of the init sequence, once per link (10 s after it came up): the ids only this layer sent, and
     * the ids only the core would send. Later writes (a tap, the battery poll) do not count.
     */
    @Synchronized
    fun checkInit(log: (String) -> Unit) {
        if (handle == 0L || !recordInit) return
        recordInit = false
        val core = nativeInitSent(handle, SystemClock.elapsedRealtime()) ?: return log("RUST TX DIFF: core failed")
        val done = core.endsWith("+")
        val rust = core.removeSuffix("+").split(',').filter { it.isNotEmpty() }.map { it.toInt(16) }.toSortedSet()
        val kotlin = initSent.toSortedSet()
        val hex = { s: Set<Int> -> s.joinToString(",") { "%04X".format(it) } }
        val parts = mutableListOf<String>()
        if (!done) parts += "core init not done"
        (kotlin - rust).takeIf { it.isNotEmpty() }?.let { parts += "only kotlin=[${hex(it)}]" }
        (rust - kotlin).takeIf { it.isNotEmpty() }?.let { parts += "only rust=[${hex(it)}]" }
        log("RUST TX DIFF: " + if (parts.isEmpty()) "none" else parts.joinToString("; "))
    }

    /** A raw wear status as the core names it (PROTOCOL.md section 13). */
    private fun wear(raw: Int) = when (raw) {
        3, 7 -> "ear"
        0, 4 -> "case"
        1, 5 -> "out"
        else -> "?"
    }

    private fun JSONObject.intOrNull(k: String): Int? = if (isNull(k)) null else optInt(k)
    private fun JSONObject.textOrNull(k: String): String? = if (isNull(k)) null else optString(k)
    private fun JSONArray?.ints(): List<Int>? = this?.let { a -> (0 until a.length()).map { a.getInt(it) } }
    private fun JSONArray?.texts(): List<String>? = this?.let { a -> (0 until a.length()).map { a.getString(it) } }

    /**
     * Each field where the core's state differs from Kotlin's, as `field kotlin=… rust=…`. [eq] and [switches]
     * (the saved `0x810D` values) are compared when given.
     */
    fun diff(k: WidgetStateStore.State, rows: Set<String>, rust: JSONObject, eq: EqState? = null,
             switches: Map<Int, Int>? = null): List<String> {
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
        same("noise", k.ancMode, rust.textOrNull("noise"))
        same("lowLatency", k.gameMode, if (rust.isNull("lowLatency")) false else rust.optBoolean("lowLatency"))
        val features = rust.optJSONArray("features").texts()?.toSet() ?: emptySet()
        (rows - features).takeIf { it.isNotEmpty() }?.let { out += "features only kotlin=${it.sorted()}" }
        (features - rows).takeIf { it.isNotEmpty() }?.let { out += "features only rust=${it.sorted()}" }
        if (eq != null) {
            same("eq", eq.current, rust.intOrNull("eq"))
            same("bass", eq.bassWave, rust.intOrNull("bass"))
            same("eqCustom", eq.custom.map { p -> "${p.id}:${p.name}:${p.gains.joinToString(",")}" },
                rust.optJSONArray("eqCustom").texts() ?: emptyList<String>())
        }
        if (switches != null) {
            val core = rust.optJSONObject("switches")?.let { o -> o.keys().asSequence().associate { it.toInt() to o.getInt(it) } } ?: emptyMap()
            for (id in (switches.keys + core.keys).sorted()) {
                same("switch 0x%02X".format(id), switches[id], core[id])
            }
        }
        return out
    }

    /** The rest of the OPPO state: the manager's fields and what [recorder] saw. Only while connected. */
    private fun more(m: BudsConnectionManager, rust: JSONObject): List<String> {
        if (!m.isConnected() || !rust.optBoolean("connected")) return emptyList()
        val out = mutableListOf<String>()
        fun same(field: String, kotlin: Any?, core: Any?) { if (kotlin != core) out += "$field kotlin=$kotlin rust=$core" }
        val s = seen
        same("alertVolume", m.alertVolume, rust.intOrNull("alert"))
        same("devices", m.devices.map { "${it.mac}|${it.name}|${it.connected}|${it.thisPhone}" },
            rust.optJSONArray("devices").texts() ?: emptyList<String>())
        if (s.preferredRead) same("preferred", s.preferred, rust.textOrNull("preferred"))
        same("spatialType", m.spatialType, rust.intOrNull("spatialType"))
        same("gameSound", m.gameSoundType, rust.intOrNull("gameSound"))
        same("gameSounds", m.gameSoundTypes, rust.optJSONArray("gameSounds").ints() ?: emptyList<Int>())
        same("smartLevel", m.smartLevel, rust.textOrNull("smart"))
        same("headMotion", m.headMotionType, rust.intOrNull("head"))
        same("codec", m.codec, rust.intOrNull("codec"))
        same("codecs", m.codecs, rust.optJSONArray("codecs").ints() ?: emptyList<Int>())
        val masks = rust.optJSONObject("holdMasks")?.let { o -> o.keys().asSequence().associate { it.toInt() to o.getInt(it) } } ?: emptyMap()
        same("holdMasks", m.lastHoldAncMasks.toSortedMap(), masks.toSortedMap())
        same("keyfn", m.keyFunctions?.map { "${it.deviceType}:${it.button}:${it.action}:${it.function}" },
            rust.optJSONArray("keyfn").texts())
        same("tapLevel", s.tap, rust.optJSONArray("tap").ints())
        same("firmware", s.firmware, rust.textOrNull("firmware"))
        same("fit", s.fit, rust.optJSONArray("fit").ints())
        same("pnc", s.pnc, rust.optJSONArray("pnc").ints())
        same("busy", synchronized(s.busy) { s.busy.toList() }, rust.optJSONArray("busy").ints()?.sorted() ?: emptyList<Int>())
        return out
    }

    private external fun nativeNew(): Long
    private external fun nativeConnected(h: Long, now: Long, name: String, product: String?, manual: String?, clearBattery: Boolean, phoneSpatial: Boolean)
    private external fun nativeBytes(h: Long, now: Long, unix: Long, data: ByteArray)
    private external fun nativeDisconnected(h: Long, now: Long)
    private external fun nativeCommon(h: Long, now: Long): String?
    private external fun nativeInitSent(h: Long, now: Long): String?
}
