// Persistence: the pairing (address, device id, K encrypted with an Android
// Keystore AES-GCM key) and the notification config (plain JSON).
package com.zenithratech.nixxa.link

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import com.zenithratech.nixxa.proto.NotifConfig
import com.zenithratech.nixxa.proto.NotifRule
import org.json.JSONArray
import org.json.JSONObject
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

data class Pairing(val address: String, val deviceId: String, val key: ByteArray)

class PairingStore(ctx: Context) {
    private val prefs = ctx.getSharedPreferences("nixxa_link", Context.MODE_PRIVATE)

    fun load(): Pairing? {
        val addr = prefs.getString("address", null) ?: return null
        val id = prefs.getString("deviceId", null) ?: return null
        val enc = prefs.getString("k", null) ?: return null
        return try {
            Pairing(addr, id, decrypt(Base64.decode(enc, Base64.NO_WRAP)))
        } catch (e: Exception) {
            Log.w(TAG, "stored key unreadable; forgetting the pairing", e)
            clear()
            null
        }
    }

    fun save(p: Pairing) {
        prefs.edit()
            .putString("address", p.address)
            .putString("deviceId", p.deviceId)
            .putString("k", Base64.encodeToString(encrypt(p.key), Base64.NO_WRAP))
            .commit()
    }

    fun clear() {
        prefs.edit().clear().commit()
    }

    private fun secretKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        g.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return g.generateKey()
    }

    /** iv(12) || ciphertext+tag */
    private fun encrypt(plain: ByteArray): ByteArray {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, secretKey())
        return c.iv + c.doFinal(plain)
    }

    private fun decrypt(blob: ByteArray): ByteArray {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(128, blob, 0, 12))
        return c.doFinal(blob, 12, blob.size - 12)
    }

    companion object {
        private const val TAG = "nixxa.store"
        private const val ALIAS = "nixxa_link_key"
    }
}

class ConfigStore(ctx: Context) {
    private val prefs = ctx.getSharedPreferences("nixxa_cfg", Context.MODE_PRIVATE)

    fun load(): NotifConfig {
        val s = prefs.getString("cfg", null) ?: return NotifConfig()
        return try {
            val o = JSONObject(s)
            val rules = o.getJSONArray("rules")
            NotifConfig(
                enabled = o.getBoolean("enabled"),
                cardSecs = o.getInt("cardSecs"),
                defaultMood = o.getInt("defaultMood"),
                quietStartMin = o.getInt("quietStartMin"),
                quietEndMin = o.getInt("quietEndMin"),
                callsInQuiet = o.getBoolean("callsInQuiet"),
                followDnd = o.getBoolean("followDnd"),
                calls = o.getBoolean("calls"),
                rules = (0 until rules.length()).map {
                    val r = rules.getJSONObject(it)
                    NotifRule(r.getString("appId"), r.getInt("mood"), r.getBoolean("show"))
                },
            )
        } catch (e: Exception) {
            Log.w("nixxa.store", "bad stored config", e)
            NotifConfig()
        }
    }

    fun save(c: NotifConfig) {
        val o = JSONObject()
        for ((k, v) in c.toMap()) if (k != "rules") o.put(k, v)
        o.put("rules", JSONArray().apply {
            c.rules.forEach { put(JSONObject().put("appId", it.appId).put("mood", it.mood).put("show", it.show)) }
        })
        prefs.edit().putString("cfg", o.toString()).apply()
    }

    /** Clock style chosen on the phone: 0 = the face, 1..20 = that clock. */
    fun clockStyle(): Int = prefs.getInt("clockStyle", 0).coerceIn(0, 20)

    fun saveClockStyle(style: Int) {
        prefs.edit().putInt("clockStyle", style.coerceIn(0, 20)).apply()
    }

    /** Packages seen posting notifications (for the app picker). */
    fun seen(): Set<String> = prefs.getStringSet("seen", emptySet()) ?: emptySet()

    fun addSeen(pkg: String) {
        val s = seen()
        if (pkg in s || s.size >= 500) return
        prefs.edit().putStringSet("seen", s + pkg).apply()
    }
}
