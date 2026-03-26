package com.fancymumble.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.OnBackPressedCallback
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

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
    }

    companion object {
        private const val REQUEST_RECORD_AUDIO = 1
    }
}