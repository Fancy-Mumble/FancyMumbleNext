package com.fancymumble.app

import android.content.Intent
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for the channel-navigation intent logic used by [MainActivity].
 *
 * [MainActivity] extends [TauriActivity] which loads the Rust native
 * library, making it impossible to instantiate via Robolectric in JVM
 * unit tests.  Instead we test the intent-handling contract directly:
 * - An intent with [MainActivity.EXTRA_CHANNEL_ID] >= 0 is a valid
 *   navigation request.
 * - After handling, the extra must be cleared to prevent re-delivery.
 */
class MainActivityTest {

    /** Replicates the logic in MainActivity.handleChannelIntent(). */
    private fun handleChannelIntent(intent: Intent?): Int? {
        val channelId = intent?.getIntExtra(MainActivity.EXTRA_CHANNEL_ID, -1) ?: -1
        return if (channelId >= 0) {
            intent?.removeExtra(MainActivity.EXTRA_CHANNEL_ID)
            channelId
        } else {
            null
        }
    }

    @Test
    fun `intent with valid channel_id returns the channel id`() {
        val intent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 42)
        }
        assertEquals(42, handleChannelIntent(intent))
    }

    @Test
    fun `intent with channel_id zero returns zero (root channel)`() {
        val intent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 0)
        }
        assertEquals(0, handleChannelIntent(intent))
    }

    @Test
    fun `intent without channel_id returns null`() {
        assertEquals(null, handleChannelIntent(Intent()))
    }

    @Test
    fun `null intent returns null`() {
        assertEquals(null, handleChannelIntent(null))
    }

    @Test
    fun `extra is cleared after handling`() {
        val intent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 7)
        }
        handleChannelIntent(intent)
        assertFalse(
            "EXTRA_CHANNEL_ID should be removed after handling",
            intent.hasExtra(MainActivity.EXTRA_CHANNEL_ID)
        )
    }

    @Test
    fun `extra not present does not crash removeExtra`() {
        val intent = Intent()
        handleChannelIntent(intent)
        assertFalse(intent.hasExtra(MainActivity.EXTRA_CHANNEL_ID))
    }

    @Test
    fun `EXTRA_CHANNEL_ID constant value is channel_id`() {
        assertEquals("channel_id", MainActivity.EXTRA_CHANNEL_ID)
    }
}
