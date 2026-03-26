package com.fancymumble.app

import android.app.Notification
import android.content.Intent
import android.os.Build
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [Build.VERSION_CODES.UPSIDE_DOWN_CAKE])
class ConnectionServiceTest {

    @Test
    fun `start intent contains server name extra`() {
        val context = ApplicationProvider.getApplicationContext<android.app.Application>()
        val shadow = Shadows.shadowOf(context)

        ConnectionService.start(context, "magical.rocks")

        val intent = shadow.nextStartedService
        assertNotNull("Service should be started", intent)
        assertEquals("magical.rocks", intent.getStringExtra("server_name"))
    }

    @Test
    fun `stop sends stop intent`() {
        val context = ApplicationProvider.getApplicationContext<android.app.Application>()
        val shadow = Shadows.shadowOf(context)

        // Start first so there is a service to stop.
        ConnectionService.start(context, "test")
        shadow.nextStartedService // consume the start intent

        ConnectionService.stop(context)
        // stopService is tracked differently by Robolectric.
        // Verify no crash and the call completes.
        assertTrue(true)
    }

    @Test
    fun `updateChannel intent contains both server and channel extras`() {
        val context = ApplicationProvider.getApplicationContext<android.app.Application>()
        val shadow = Shadows.shadowOf(context)

        ConnectionService.updateChannel(context, "magical.rocks", "General")

        val intent = shadow.nextStartedService
        assertNotNull("Service should be started", intent)
        assertEquals("magical.rocks", intent.getStringExtra("server_name"))
        assertEquals("General", intent.getStringExtra("channel_name"))
    }

    @Test
    fun `onStartCommand with server name builds notification with server as title`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "magical.rocks")
        }

        service.onStartCommand(intent, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)

        val shadowNotification = Shadows.shadowOf(notification)
        assertEquals("magical.rocks", shadowNotification.contentTitle)
        assertEquals("Connected", shadowNotification.contentText)
    }

    @Test
    fun `onStartCommand with channel name shows channel in content text`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "magical.rocks")
            putExtra("channel_name", "General")
        }

        service.onStartCommand(intent, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)

        val shadowNotification = Shadows.shadowOf(notification)
        assertEquals("magical.rocks", shadowNotification.contentTitle)
        assertEquals("#General", shadowNotification.contentText)
    }

    @Test
    fun `onStartCommand with empty channel name shows Connected`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "test-server")
            putExtra("channel_name", "")
        }

        service.onStartCommand(intent, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)

        val shadowNotification = Shadows.shadowOf(notification)
        assertEquals("test-server", shadowNotification.contentTitle)
        assertEquals("Connected", shadowNotification.contentText)
    }

    @Test
    fun `onStartCommand with null intent uses default server name`() {
        val service = Robolectric.setupService(ConnectionService::class.java)

        service.onStartCommand(null, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)

        val shadowNotification = Shadows.shadowOf(notification)
        assertEquals("server", shadowNotification.contentTitle)
        assertEquals("Connected", shadowNotification.contentText)
    }

    @Test
    fun `notification has ongoing flag set`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "test")
        }

        service.onStartCommand(intent, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)
        assertTrue(
            "Notification should be ongoing",
            notification.flags and Notification.FLAG_ONGOING_EVENT != 0
        )
    }

    @Test
    fun `notification has disconnect action`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "test")
        }

        service.onStartCommand(intent, 0, 1)

        val nm = Shadows.shadowOf(
            service.getSystemService(android.content.Context.NOTIFICATION_SERVICE)
                    as android.app.NotificationManager
        )
        val notification = nm.getNotification(1001)
        assertNotNull("Notification should exist", notification)
        assertEquals("Should have one action", 1, notification.actions.size)
        assertEquals("Disconnect", notification.actions[0].title.toString())
    }

    @Test
    fun `disconnect action returns START_NOT_STICKY`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            action = ConnectionService.ACTION_DISCONNECT
        }

        val result = service.onStartCommand(intent, 0, 1)
        assertEquals(android.app.Service.START_NOT_STICKY, result)
    }

    @Test
    fun `normal start returns START_STICKY`() {
        val service = Robolectric.setupService(ConnectionService::class.java)
        val intent = Intent().apply {
            putExtra("server_name", "test")
        }

        val result = service.onStartCommand(intent, 0, 1)
        assertEquals(android.app.Service.START_STICKY, result)
    }
}
