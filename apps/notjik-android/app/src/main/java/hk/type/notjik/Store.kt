package hk.type.notjik

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import org.json.JSONObject
import java.util.UUID

class Store(context: Context, name: String = "notifications.db") : SQLiteOpenHelper(context, name, null, 1) {
    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL("CREATE TABLE cards(id TEXT PRIMARY KEY, payload TEXT NOT NULL, received INTEGER NOT NULL, displayed INTEGER DEFAULT 0)")
        db.execSQL("CREATE TABLE outbox(id TEXT PRIMARY KEY, payload TEXT NOT NULL)")
    }
    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) = Unit

    // Read and dismissal are monotonic in the authority protocol. Preserve local
    // acknowledgments even if an older in-flight history request finishes later.
    fun save(card: JSONObject): JSONObject = synchronized(lock) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            val merged = JSONObject(card.toString())
            val previous = find(db, card.getString("id"))
            merged.put("read", merged.optBoolean("read") || previous?.optBoolean("read") == true)
            merged.put("dismissed", merged.optBoolean("dismissed") || previous?.optBoolean("dismissed") == true)
            val exists = previous != null
            if (exists) db.execSQL("UPDATE cards SET payload=?,received=? WHERE id=?", arrayOf(merged.toString(), merged.optLong("updated", System.currentTimeMillis()/1000), merged.getString("id")))
            else db.execSQL("INSERT INTO cards(id,payload,received) VALUES(?,?,?)", arrayOf(merged.getString("id"), merged.toString(), merged.optLong("updated", System.currentTimeMillis()/1000)))
            db.setTransactionSuccessful()
            merged
        } finally { db.endTransaction() }
    }
    fun cards(): List<JSONObject> = synchronized(lock) {
        readableDatabase.rawQuery("SELECT payload FROM cards ORDER BY received DESC,id", null).use { c ->
            buildList { while(c.moveToNext()) add(JSONObject(c.getString(0))) }
        }
    }
    fun revision(id: String): Int = synchronized(lock) {
        readableDatabase.rawQuery("SELECT displayed FROM cards WHERE id=?", arrayOf(id)).use { if(it.moveToFirst()) it.getInt(0) else 0 }
    }
    fun displayed(id: String, revision: Int) = synchronized(lock) {
        writableDatabase.execSQL("UPDATE cards SET displayed=? WHERE id=?", arrayOf(revision,id))
    }
    fun enqueue(value: JSONObject) = synchronized(lock) {
        val db = writableDatabase
        db.beginTransaction()
        try {
            val id = UUID.randomUUID().toString()
            val payload = JSONObject(value.toString()).put("request_id", id)
            db.execSQL("INSERT INTO outbox VALUES(?,?)", arrayOf(id, payload.toString()))
            if (value.optString("op") in listOf("read", "dismiss")) mark(db, value.getString("id"), value.getString("op") == "dismiss")
            db.setTransactionSuccessful()
        } finally { db.endTransaction() }
    }
    fun pending(): List<JSONObject> = synchronized(lock) {
        readableDatabase.rawQuery("SELECT payload FROM outbox ORDER BY rowid", null).use { c -> buildList { while(c.moveToNext()) add(JSONObject(c.getString(0))) } }
    }
    fun sent(id: String) = synchronized(lock) { writableDatabase.execSQL("DELETE FROM outbox WHERE id=?", arrayOf(id)) }
    fun mark(id: String, dismiss: Boolean) = synchronized(lock) { mark(writableDatabase, id, dismiss) }
    private fun mark(db: SQLiteDatabase, id: String, dismiss: Boolean) {
        find(db, id)?.let { card ->
            if (dismiss) card.put("dismissed", true) else card.put("read", true)
            db.execSQL("UPDATE cards SET payload=? WHERE id=?", arrayOf(card.toString(), id))
        }
    }
    private fun find(db: SQLiteDatabase, id: String): JSONObject? = db.rawQuery("SELECT payload FROM cards WHERE id=?", arrayOf(id)).use {
        if (it.moveToFirst()) JSONObject(it.getString(0)) else null
    }
    companion object { private val lock = Any() }
}
