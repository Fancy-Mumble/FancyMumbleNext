package com.fancymumble.app

import android.app.Activity
import android.util.Log
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.firebase.messaging.FirebaseMessaging

@InvokeArg
internal class TopicArgs {
    var topic: String = ""
}

@InvokeArg
internal class TopicListArgs {
    var topics: Array<String> = emptyArray()
}

/**
 * Tauri plugin for managing FCM topic subscriptions.
 *
 * Rust calls `run_mobile_plugin("subscribeTopic", ...)` and
 * `run_mobile_plugin("unsubscribeTopic", ...)` to control which
 * FCM topics the device listens to.
 *
 * The Mumble server sends push notifications to topics named
 * `mumble_server<N>_channel<C>`. When the client connects and
 * receives channel state, Rust subscribes to the matching topics
 * so the device receives background push notifications.
 */
@TauriPlugin
class FcmPlugin(private val activity: Activity) : Plugin(activity) {

    companion object {
        private const val TAG = "FcmPlugin"
        private var instance: FcmPlugin? = null

        /**
         * Called by [FcmService] when the FCM token is refreshed.
         * Can be used in the future to emit an event to Rust.
         */
        fun onTokenRefreshed(token: String) {
            Log.d(TAG, "Token refreshed (length=${token.length})")
        }
    }

    override fun load(webView: WebView) {
        super.load(webView)
        instance = this
    }

    @Command
    fun subscribeTopic(invoke: Invoke) {
        val args = invoke.parseArgs(TopicArgs::class.java)
        val topic = args.topic
        if (topic.isBlank()) {
            invoke.reject("topic must not be blank")
            return
        }
        Log.d(TAG, "Subscribing to FCM topic: $topic")
        FirebaseMessaging.getInstance().subscribeToTopic(topic)
            .addOnCompleteListener { task ->
                if (task.isSuccessful) {
                    Log.d(TAG, "Subscribed to topic: $topic")
                } else {
                    Log.w(TAG, "Failed to subscribe to topic: $topic", task.exception)
                }
            }
        invoke.resolve()
    }

    @Command
    fun unsubscribeTopic(invoke: Invoke) {
        val args = invoke.parseArgs(TopicArgs::class.java)
        val topic = args.topic
        if (topic.isBlank()) {
            invoke.reject("topic must not be blank")
            return
        }
        Log.d(TAG, "Unsubscribing from FCM topic: $topic")
        FirebaseMessaging.getInstance().unsubscribeFromTopic(topic)
            .addOnCompleteListener { task ->
                if (task.isSuccessful) {
                    Log.d(TAG, "Unsubscribed from topic: $topic")
                } else {
                    Log.w(TAG, "Failed to unsubscribe from topic: $topic", task.exception)
                }
            }
        invoke.resolve()
    }

    @Command
    fun subscribeTopics(invoke: Invoke) {
        val args = invoke.parseArgs(TopicListArgs::class.java)
        val messaging = FirebaseMessaging.getInstance()
        for (topic in args.topics) {
            if (topic.isBlank()) continue
            Log.d(TAG, "Subscribing to FCM topic: $topic")
            messaging.subscribeToTopic(topic)
                .addOnCompleteListener { task ->
                    if (task.isSuccessful) {
                        Log.d(TAG, "Subscribed to topic: $topic")
                    } else {
                        Log.w(TAG, "Failed to subscribe to topic: $topic", task.exception)
                    }
                }
        }
        invoke.resolve()
    }

    @Command
    fun unsubscribeAll(invoke: Invoke) {
        Log.d(TAG, "Deleting FCM instance ID (unsubscribes from all topics)")
        FirebaseMessaging.getInstance().deleteToken()
            .addOnCompleteListener { task ->
                if (task.isSuccessful) {
                    Log.d(TAG, "Token deleted, all topic subscriptions cleared")
                } else {
                    Log.w(TAG, "Failed to delete token", task.exception)
                }
            }
        invoke.resolve()
    }

    @Command
    fun getToken(invoke: Invoke) {
        FirebaseMessaging.getInstance().token
            .addOnCompleteListener { task ->
                if (task.isSuccessful) {
                    val result = JSObject()
                    result.put("token", task.result)
                    Log.d(TAG, "FCM token obtained (length=${task.result.length})")
                    invoke.resolve(result)
                } else {
                    Log.w(TAG, "Failed to get FCM token", task.exception)
                    invoke.reject("Failed to get FCM token")
                }
            }
    }
}
