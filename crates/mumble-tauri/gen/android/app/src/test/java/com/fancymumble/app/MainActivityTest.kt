package com.fancymumble.app

import android.content.Intent
import android.os.Build
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [Build.VERSION_CODES.UPSIDE_DOWN_CAKE])
class MainActivityTest {

    @Test
    fun `intent with channel_id extra triggers navigation`() {
        val intent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 42)
        }
        val activity = Robolectric.buildActivity(MainActivity::class.java, intent)
            .create()
            .get()

        // After handleChannelIntent in onCreate, the extra should be cleared
        // to prevent re-navigation on re-delivery.
        assertEquals(
            "channel_id extra should be cleared after handling",
            -1,
            activity.intent.getIntExtra(MainActivity.EXTRA_CHANNEL_ID, -1)
        )
    }

    @Test
    fun `intent without channel_id extra does not crash`() {
        val activity = Robolectric.buildActivity(MainActivity::class.java)
            .create()
            .get()

        // Should not crash when no channel_id is present.
        assertEquals(
            "channel_id should be absent",
            -1,
            activity.intent.getIntExtra(MainActivity.EXTRA_CHANNEL_ID, -1)
        )
    }

    @Test
    fun `onNewIntent with channel_id clears the extra`() {
        val controller = Robolectric.buildActivity(MainActivity::class.java)
            .create()

        val newIntent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 7)
        }
        controller.newIntent(newIntent)

        assertEquals(
            "channel_id extra should be cleared after onNewIntent handling",
            -1,
            newIntent.getIntExtra(MainActivity.EXTRA_CHANNEL_ID, -1)
        )
    }

    @Test
    fun `onNewIntent without channel_id does not crash`() {
        val controller = Robolectric.buildActivity(MainActivity::class.java)
            .create()

        controller.newIntent(Intent())
        // No crash = pass
    }

    @Test
    fun `channel_id of zero is treated as valid`() {
        val intent = Intent().apply {
            putExtra(MainActivity.EXTRA_CHANNEL_ID, 0)
        }
        val activity = Robolectric.buildActivity(MainActivity::class.java, intent)
            .create()
            .get()

        // channel_id 0 is valid (root channel), so it should be handled
        // and cleared.
        assertEquals(
            "channel_id 0 should be cleared after handling",
            -1,
            activity.intent.getIntExtra(MainActivity.EXTRA_CHANNEL_ID, -1)
        )
    }
}
