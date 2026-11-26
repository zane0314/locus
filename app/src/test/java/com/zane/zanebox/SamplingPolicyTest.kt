package com.zane.zanebox
import com.zane.zanebox.runtime.SamplingPolicy
import com.zane.zanebox.runtime.TrafficSamples
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
class SamplingPolicyTest {
    @Test fun screenAndObserversControlRateButAllRouteTargetsAreCounted() {
        assertEquals(1000L,SamplingPolicy.interval(true,true))
        assertEquals(10000L,SamplingPolicy.interval(false,true));assertEquals(10000L,SamplingPolicy.interval(true,false))
        assertEquals(60000L,SamplingPolicy.interval(false,false))
        assertEquals(5000L,SamplingPolicy.controllerPollInterval(true,true))
        assertEquals(30000L,SamplingPolicy.controllerPollInterval(true,false))
        assertEquals(30000L,SamplingPolicy.controllerPollInterval(false,true))
        assertEquals(120000L,SamplingPolicy.controllerPollInterval(false,false))
        assertEquals(60000L,SamplingPolicy.persistInterval(true,true))
        assertEquals(60000L,SamplingPolicy.persistInterval(true,false))
        assertEquals(60000L,SamplingPolicy.persistInterval(false,true))
        assertEquals(180000L,SamplingPolicy.persistInterval(false,false))
        assertEquals(12000L,SamplingPolicy.coalescedAutoTestDelay(listOf(1000L,12000L,90000L),300_000L))
        assertEquals(1000L,SamplingPolicy.coalescedAutoTestDelay(listOf(1000L,12000L),30_000L))
        assertEquals(42_000L,SamplingPolicy.coalescedAutoTestDelay(listOf(42_000L),300_000L))
        assertNull(SamplingPolicy.coalescedAutoTestDelay(listOf(-1L),300_000L))
        val config=JSONObject("""{"route":{"final":"proxy","rules":[{"outbound":"direct"},{"type":"logical","rules":[{"outbound":"smart-google"}]}]},"outbounds":[{"tag":"node-1"},{"tag":"node-2"}]}""")
        assertEquals(listOf("proxy","direct","smart-google"),SamplingPolicy.trackedTags(config))
    }
    @Test fun idleDoesNotTurnOffWireGuardOrBackgroundSubscriptions() {
        assertTrue(SamplingPolicy.pauseCore(true,false,false))
        assertFalse(SamplingPolicy.pauseCore(true,false,true));assertFalse(SamplingPolicy.pauseCore(true,true,false));assertFalse(SamplingPolicy.pauseCore(false,false,false))
    }
    @Test fun statisticsKeepMoreThan512ConnectionsAnd256KiB() {
        val rows=(1..600).joinToString(","){"""{"id":"$it","upload":1,"metadata":{"host":"example.com","processPath":"${"a".repeat(450)}"}}"""}
        val text="{\"connections\":[$rows]}";assertTrue(text.length>256*1024)
        assertEquals(600,TrafficSamples.parse(text).size)
    }
}
