package com.fancymumble.app

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

/**
 * Handles incoming FCM push notifications and token refresh events.
 *
 * The Mumble server sends push notifications via FCM HTTP v1 API
 * directly to this device's registration token. This service receives
 * them when the app is in the foreground and shows a system
 * notification. When the app is in the background, FCM auto-displays
 * the `notification` payload using the default notification channel
 * set in AndroidManifest.xml.
 *
 * Token refresh events are forwarded to [FcmPlugin] so the Rust
 * backend can react if needed in the future.
 */
class FcmService : FirebaseMessagingService() {

    companion object {
        private const val TAG = "FcmService"
        const val CHANNEL_ID = "messages"
        private var nextNotificationId = 3000

        /**
         * Ensures the "messages" notification channel exists.
         * Safe to call multiple times — Android no-ops if it already exists.
         */
        fun ensureChannel(context: android.content.Context) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val channel = NotificationChannel(
                    CHANNEL_ID,
                    "Messages",
                    NotificationManager.IMPORTANCE_HIGH
                ).apply {
                    description = "Chat message notifications from Mumble"
                }
                val manager = context.getSystemService(NotificationManager::class.java)
                manager.createNotificationChannel(channel)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        ensureChannel(applicationContext)
    }

    override fun onNewToken(token: String) {
        super.onNewToken(token)
        Log.d(TAG, "FCM token refreshed")
        FcmPlugin.onTokenRefreshed(token)
    }

    override fun onMessageReceived(message: RemoteMessage) {
        super.onMessageReceived(message)
        Log.d(TAG, "FCM message received from: ${message.from}")

        val title = message.notification?.title
            ?: message.data["title"]
            ?: "Mumble"
        val body = message.notification?.body
            ?: message.data["body"]
            ?: ""
        val channelIdStr = message.data["channel_id"]
        val channelId = channelIdStr?.toIntOrNull()

        showNotification(title, body, channelId)
    }

    private fun showNotification(title: String, body: String, channelId: Int?) {
        val context = applicationContext

        val builder = NotificationCompat.Builder(context, "messages")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(title)
            .setContentText(body)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)

        channelId?.let { chId ->
            val tapIntent = Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
                putExtra(MainActivity.EXTRA_CHANNEL_ID, chId)
            }
            val tapPending = PendingIntent.getActivity(
                context,
                chId,
                tapIntent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            builder.setContentIntent(tapPending)
        }

        NotificationManagerCompat.from(context)
            .notify(nextNotificationId, builder.build())

        nextNotificationId = if (nextNotificationId >= 3999) 3000 else nextNotificationId + 1
    }
}
