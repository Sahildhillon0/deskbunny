package com.zenithratech.nixxa

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    private var channel: LinkChannel? = null
    private var permCallback: ((Boolean) -> Unit)? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        channel = LinkChannel(this, flutterEngine.dartExecutor.binaryMessenger)
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        channel?.dispose()
        channel = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    /** Bluetooth permissions are required (false when denied); POST_NOTIFICATIONS is asked for too but optional. */
    fun requestLinkPermissions(cb: (Boolean) -> Unit) {
        val required = if (Build.VERSION.SDK_INT >= 31) {
            listOf(Manifest.permission.BLUETOOTH_SCAN, Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            listOf(Manifest.permission.ACCESS_FINE_LOCATION) // BLE scans need location before Android 12
        }
        val optional = if (Build.VERSION.SDK_INT >= 33) listOf(Manifest.permission.POST_NOTIFICATIONS) else emptyList()
        val missing = (required + optional).filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) return cb(true)
        permCallback?.invoke(false)
        permCallback = { cb(requiredGranted(required)) }
        requestPermissions(missing.toTypedArray(), REQ_PERMS)
    }

    private fun requiredGranted(required: List<String>) =
        required.all { ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQ_PERMS) return
        val cb = permCallback
        permCallback = null
        cb?.invoke(true) // the callback re-checks what was actually granted
    }

    companion object {
        private const val REQ_PERMS = 0x6E78
    }
}
