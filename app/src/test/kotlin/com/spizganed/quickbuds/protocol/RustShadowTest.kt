package com.spizganed.quickbuds.protocol

import com.spizganed.quickbuds.widget.WidgetStateStore
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Test

class RustShadowTest {
    private val kotlin = WidgetStateStore.State(ancMode = "Transparency", leftBattery = 70, rightBattery = 80,
        leftStatus = 3, rightStatus = 4, connected = true)

    private fun rust(left: Int = 70, features: String = "\"anc\",\"earbuds\"") = JSONObject(
        """{"connected":true,"battery":[$left,80,-1],"wear":["ear","case"],"noise":"Transparency","lowLatency":null,"features":[$features]}""")

    @Test
    fun the_same_state_has_no_difference() {
        assertEquals(emptyList<String>(), RustShadow.diff(kotlin, setOf("anc", "earbuds"), rust()))
    }

    @Test
    fun each_field_that_differs_is_named() {
        assertEquals(listOf("battery.left kotlin=70 rust=60", "features only rust=[eq]"),
            RustShadow.diff(kotlin, setOf("anc", "earbuds"), rust(left = 60, features = "\"anc\",\"earbuds\",\"eq\"")))
    }

    @Test
    fun with_no_link_only_the_connection_counts() {
        assertEquals(listOf("connected kotlin=false rust=true"),
            RustShadow.diff(kotlin.copy(connected = false), emptySet(), rust()))
    }
}
