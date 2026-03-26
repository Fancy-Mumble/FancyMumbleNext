package com.fancymumble.app

import android.app.Activity
import android.app.PendingIntent
import android.content.Intent
import android.graphics.BitmapFactory
import android.util.Base64
import android.util.Log
import android.webkit.WebView
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
internal class ServiceArgs {
    var serverName: String = "server"
}

@InvokeArg
internal class ServiceChannelArgs {
    var serverName: String = "server"
    var channelName: String = ""
}

@InvokeArg
internal class ChatNotificationArgs {
    var title: String = ""
    var body: String = ""
    var iconBase64: String? = null
    var channelId: Int? = null
}

/**
 * Tauri plugin that bridges the Rust backend to the Android
 * [ConnectionService] foreground service.
 *
 * Rust calls `run_mobile_plugin("startService", ...)` /
 * `run_mobile_plugin("stopService", ...)` to control the
 * service lifecycle when connecting to / disconnecting from a
 * Mumble server.
 *
 * `showChatNotification` is called for every incoming chat message
 * so that the large-icon (sender avatar) can be decoded from raw
 * bytes inside the app process.
 *
 * The notification disconnect button uses `trigger("disconnect-requested")`
 * to send an event through the Tauri channel system back to Rust,
 * which then calls `AppState::disconnect()`.
 */
@TauriPlugin
class ConnectionServicePlugin(private val activity: Activity) : Plugin(activity) {

    companion object {
        private const val TAG = "ConnectionServicePlugin"
        private var instance: ConnectionServicePlugin? = null

        /**
         * Called from [ConnectionService] when the user taps the
         * "Disconnect" action on the foreground-service notification.
         *
         * Triggers the "disconnect-requested" event which is listened
         * to on the Rust side via a registered [Channel].
         */
        fun requestDisconnect() {
            instance?.trigger("disconnect-requested", JSObject())
        }

        /**
         * Called from [MainActivity] when the user taps a chat
         * notification.  Triggers "navigate-to-channel" which the
         * Rust side forwards as a Tauri event to the frontend.
         */
        fun navigateToChannel(channelId: Int) {
            instance?.trigger("navigate-to-channel", JSObject().put("channelId", channelId))
        }
    }

    // Notification IDs 2000-2999 are reserved for chat messages so they
    // never collide with the foreground-service notification (1001).
    private var nextNotificationId = 2000

    override fun load(webView: WebView) {
        super.load(webView)
        instance = this
    }

    @Command
    fun startService(invoke: Invoke) {
        val args = invoke.parseArgs(ServiceArgs::class.java)
        Log.d(TAG, "startService serverName='${args.serverName}'")
        ConnectionService.start(activity, args.serverName)
        invoke.resolve()
    }

    @Command
    fun stopService(invoke: Invoke) {
        ConnectionService.stop(activity)
        invoke.resolve()
    }

    @Command
    fun showChatNotification(invoke: Invoke) {
        val args = invoke.parseArgs(ChatNotificationArgs::class.java)
        val context = activity.applicationContext

        val builder = NotificationCompat.Builder(context, "messages")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentTitle(args.title)
            .setContentText(args.body)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)

        // Tapping the notification brings up the app and navigates to the
        // channel that received the message.
        args.channelId?.let { chId ->
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

        args.iconBase64?.let { b64 ->
            runCatching {
                val bytes = Base64.decode(b64, Base64.DEFAULT)
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
            }.getOrNull()?.let { bitmap ->
                builder.setLargeIcon(bitmap)
            }
        }

        NotificationManagerCompat.from(context)
            .notify(nextNotificationId, builder.build())

        nextNotificationId = if (nextNotificationId >= 2999) 2000 else nextNotificationId + 1
        invoke.resolve()
    }

    @Command
    fun updateServiceChannel(invoke: Invoke) {
        val args = invoke.parseArgs(ServiceChannelArgs::class.java)
        Log.d(TAG, "updateServiceChannel serverName='${args.serverName}' channelName='${args.channelName}'")
        ConnectionService.updateChannel(activity, args.serverName, args.channelName)
        invoke.resolve()
    }
}

