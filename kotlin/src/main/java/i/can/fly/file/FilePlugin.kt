package i.can.fly.file

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream

@InvokeArg
class ApplicationIconArgs {
	var packageName: String = ""
}

@TauriPlugin
class FilePlugin(private val activity: Activity) : Plugin(activity) {
	/** 按包名读取应用图标，统一绘制成 96 × 96 PNG，兼容自适应和矢量图标。 */
	@Command
	fun getApplicationIcon(invoke: Invoke) {
		try {
			val args = invoke.parseArgs(ApplicationIconArgs::class.java)
			require(args.packageName.isNotBlank()) { "packageName must not be empty" }
			val drawable = activity.packageManager.getApplicationIcon(args.packageName).mutate()
			val bitmap = Bitmap.createBitmap(96, 96, Bitmap.Config.ARGB_8888)
			val originalBounds = drawable.copyBounds()
			val dataUrl = try {
				drawable.setBounds(0, 0, bitmap.width, bitmap.height)
				drawable.draw(Canvas(bitmap))
				ByteArrayOutputStream().use { output ->
					check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) { "Cannot encode application icon" }
					"data:image/png;base64," + Base64.encodeToString(output.toByteArray(), Base64.NO_WRAP)
				}
			} finally {
				drawable.bounds = originalBounds
				bitmap.recycle()
			}
			invoke.resolve(JSObject().apply { put("dataUrl", dataUrl) })
		} catch (error: Exception) {
			invoke.reject(error.message ?: "Cannot load application icon")
		}
	}

	/** 列出当前用户可见、有桌面入口的应用。 */
	@Suppress("DEPRECATION")
	@Command
	fun listApplications(invoke: Invoke) {
		try {
			val pm = activity.packageManager
			val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
			val applications = pm.queryIntentActivities(intent, 0)
				.map { it.activityInfo.applicationInfo }
				.distinctBy { it.packageName }
				.filter { it.packageName != activity.packageName }
				.map { info ->
					Pair(pm.getApplicationLabel(info).toString(), info.packageName)
				}
				.sortedWith(compareBy<Pair<String, String>> { it.first }.thenBy { it.second })
			val items = JSONArray()
			applications.forEach { (name, packageName) ->
				items.put(JSONObject().put("platform", "android").put("name", name).put("packageName", packageName))
			}
			invoke.resolve(JSObject().apply { put("applications", items) })
		} catch (error: Exception) {
			invoke.reject(error.message ?: "Cannot list applications")
		}
	}
}
