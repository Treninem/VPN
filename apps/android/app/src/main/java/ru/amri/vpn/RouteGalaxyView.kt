package ru.amri.vpn

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.view.View
import kotlin.math.min

/**
 * Privacy-safe live map surface. Labels are rendered locally and never exported.
 * Real routes can replace the empty state as soon as the Rust routing snapshot is wired.
 */
class RouteGalaxyView(context: Context) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private var controllerState = VpnControllerState.IDLE

    init {
        isClickable = true
        isFocusable = true
        minimumHeight = dp(246)
    }

    fun updateControllerState(state: VpnControllerState) {
        controllerState = state
        invalidate()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val width = MeasureSpec.getSize(widthMeasureSpec)
        setMeasuredDimension(width, resolveSize(dp(246), heightMeasureSpec))
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val bounds = RectF(0f, 0f, width.toFloat(), height.toFloat())
        paint.color = Color.argb(232, 4, 18, 43)
        canvas.drawRoundRect(bounds, dp(24).toFloat(), dp(24).toFloat(), paint)

        paint.style = Paint.Style.STROKE
        paint.strokeWidth = dp(1).toFloat()
        paint.color = Color.argb(120, 34, 202, 242)
        canvas.drawRoundRect(bounds, dp(24).toFloat(), dp(24).toFloat(), paint)

        val cx = width / 2f
        val cy = height / 2f + dp(5)
        val base = min(width, height).toFloat()
        for (factor in listOf(0.22f, 0.34f, 0.44f)) {
            paint.color = Color.argb(58, 58, 170, 239)
            canvas.drawCircle(cx, cy, base * factor, paint)
        }

        paint.style = Paint.Style.FILL
        paint.color = Color.rgb(8, 91, 178)
        canvas.drawCircle(cx, cy, dp(38).toFloat(), paint)
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = dp(2).toFloat()
        paint.color = Color.rgb(58, 225, 242)
        canvas.drawCircle(cx, cy, dp(38).toFloat(), paint)
        drawCentered(canvas, "AMRI", cx, cy + dp(5), 15f, Color.WHITE)

        val ready = controllerState == VpnControllerState.SERVICE_READY
        val nodes = listOf(
            GalaxyNode(-0.34f, -0.22f, "ПУЛ", "0 узлов"),
            GalaxyNode(0.34f, -0.20f, "PROBE", if (ready) "служба готова" else "ожидание"),
            GalaxyNode(-0.32f, 0.26f, "ROUTES", "0 активно"),
            GalaxyNode(0.34f, 0.25f, "DIRECT", "готов"),
        )
        nodes.forEach { node ->
            val x = cx + width * node.x
            val y = cy + height * node.y
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = dp(1).toFloat()
            paint.color = Color.argb(95, 37, 186, 239)
            canvas.drawLine(cx, cy, x, y, paint)
            paint.style = Paint.Style.FILL
            paint.color = Color.rgb(9, 36, 72)
            canvas.drawCircle(x, y, dp(24).toFloat(), paint)
            paint.style = Paint.Style.STROKE
            paint.color = Color.argb(150, 64, 202, 242)
            canvas.drawCircle(x, y, dp(24).toFloat(), paint)
            drawCentered(canvas, node.title, x, y - dp(2), 10f, Color.WHITE)
            drawCentered(canvas, node.value, x, y + dp(11), 7.5f, Color.rgb(170, 180, 196))
        }

        paint.style = Paint.Style.FILL
        paint.textAlign = Paint.Align.LEFT
        paint.textSize = sp(12f)
        paint.color = Color.rgb(65, 221, 239)
        canvas.drawText("AMRI ROUTE GALAXY", dp(18).toFloat(), dp(25).toFloat(), paint)
        paint.textSize = sp(9.5f)
        paint.color = Color.rgb(175, 189, 209)
        canvas.drawText(
            "назначение → отдельный маршрут → VPN / DIRECT",
            dp(18).toFloat(),
            height - dp(14).toFloat(),
            paint,
        )
    }

    private fun drawCentered(
        canvas: Canvas,
        value: String,
        x: Float,
        y: Float,
        size: Float,
        color: Int,
    ) {
        paint.style = Paint.Style.FILL
        paint.textAlign = Paint.Align.CENTER
        paint.textSize = sp(size)
        paint.color = color
        canvas.drawText(value, x, y, paint)
    }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()
    private fun sp(value: Float): Float = value * resources.displayMetrics.scaledDensity

    private data class GalaxyNode(
        val x: Float,
        val y: Float,
        val title: String,
        val value: String,
    )
}
