package com.fancymumble.app

import android.os.Bundle
import androidx.activity.OnBackPressedCallback

class MainActivity : TauriActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Consume the system back gesture (swipe-from-edge) so the WebView
        // never receives history.back().  This prevents accidental disconnects
        // when the user swipes from the left edge on Android.
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                // Intentionally empty: swallow the back event.
            }
        })
    }
}