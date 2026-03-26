package com.fancymumble.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * Tests for the @InvokeArg data classes used by the Tauri plugin bridge.
 * These verify that default values are correct so the Kotlin side falls
 * back gracefully when optional fields are missing.
 */
class InvokeArgDefaultsTest {

    @Test
    fun `ServiceArgs defaults serverName to server`() {
        val args = ServiceArgs()
        assertEquals("server", args.serverName)
    }

    @Test
    fun `ServiceArgs serverName can be overwritten`() {
        val args = ServiceArgs()
        args.serverName = "magical.rocks"
        assertEquals("magical.rocks", args.serverName)
    }

    @Test
    fun `ServiceChannelArgs defaults serverName to server`() {
        val args = ServiceChannelArgs()
        assertEquals("server", args.serverName)
    }

    @Test
    fun `ServiceChannelArgs defaults channelName to empty`() {
        val args = ServiceChannelArgs()
        assertEquals("", args.channelName)
    }

    @Test
    fun `ServiceChannelArgs fields can be overwritten`() {
        val args = ServiceChannelArgs()
        args.serverName = "magical.rocks"
        args.channelName = "General"
        assertEquals("magical.rocks", args.serverName)
        assertEquals("General", args.channelName)
    }

    @Test
    fun `ChatNotificationArgs defaults title to empty`() {
        val args = ChatNotificationArgs()
        assertEquals("", args.title)
    }

    @Test
    fun `ChatNotificationArgs defaults body to empty`() {
        val args = ChatNotificationArgs()
        assertEquals("", args.body)
    }

    @Test
    fun `ChatNotificationArgs defaults iconBase64 to null`() {
        val args = ChatNotificationArgs()
        assertNull(args.iconBase64)
    }

    @Test
    fun `ChatNotificationArgs defaults channelId to null`() {
        val args = ChatNotificationArgs()
        assertNull(args.channelId)
    }

    @Test
    fun `ChatNotificationArgs fields can be set`() {
        val args = ChatNotificationArgs()
        args.title = "Alice"
        args.body = "Hello!"
        args.iconBase64 = "iVBOR..."
        args.channelId = 5
        assertEquals("Alice", args.title)
        assertEquals("Hello!", args.body)
        assertEquals("iVBOR...", args.iconBase64)
        assertEquals(5, args.channelId)
    }
}
