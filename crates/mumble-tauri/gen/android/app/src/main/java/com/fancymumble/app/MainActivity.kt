package com.fancymumble.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.OnBackPressedCallback
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {

    companion object {
        private const val REQUEST_RECORD_AUDIO = 1
        const val EXTRA_CHANNEL_ID = "channel_id"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Ensure the "messages" notification channel exists before any
        // FCM message can arrive (required on Android 8+).
        FcmService.ensureChannel(this)

        // Request microphone permission at launch so the Oboe audio
        // capture stream can be opened when the user unmutes.
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(Manifest.permission.RECORD_AUDIO),
                REQUEST_RECORD_AUDIO
            )
        }

        // Consume the system back gesture (swipe-from-edge) so the WebView
        // never receives history.back().  This prevents accidental disconnects
        // when the user swipes from the left edge on Android.
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                // Intentionally empty: swallow the back event.
            }
        })

        // Handle channel navigation from notification tap at cold start.
        handleChannelIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleChannelIntent(intent)
    }

    /**
     * If the intent carries an [EXTRA_CHANNEL_ID], notify the Rust
     * backend via the Tauri plugin so the frontend can navigate to
     * the correct channel.
     */
    private fun handleChannelIntent(intent: Intent?) {
        val channelId = intent?.getIntExtra(EXTRA_CHANNEL_ID, -1) ?: -1
        if (channelId >= 0) {
            ConnectionServicePlugin.navigateToChannel(channelId)
            // Clear the extra so re-delivery does not re-navigate.
            intent?.removeExtra(EXTRA_CHANNEL_ID)
        }
    }
}