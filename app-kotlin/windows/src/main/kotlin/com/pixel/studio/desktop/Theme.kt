package com.pixel.studio.desktop

import java.awt.Color
import java.awt.Font
import javax.swing.BorderFactory
import javax.swing.JButton
import javax.swing.UIManager
import javax.swing.border.Border

/** Dark neon theme matching the Pixel Studio icon (cyan → violet on deep navy). */
object Theme {
    val BG = Color(0x0B, 0x10, 0x24)
    val PANEL = Color(0x12, 0x18, 0x33)
    val PANEL_LIGHT = Color(0x1B, 0x23, 0x47)
    val CANVAS_BG = Color(0x08, 0x0C, 0x1C)
    val TEXT = Color(0xE6, 0xEA, 0xFF)
    val TEXT_DIM = Color(0x8C, 0x95, 0xC4)
    val ACCENT = Color(0x38, 0xE1, 0xFF)
    val ACCENT_2 = Color(0xA8, 0x55, 0xF7)
    val ACCENT_DIM = Color(0x38, 0xE1, 0xFF, 90)
    val GRID = Color(255, 255, 255, 28)
    val CHECKER_A = Color(0x2A, 0x2F, 0x45)
    val CHECKER_B = Color(0x22, 0x27, 0x3B)

    fun install() {
        runCatching { UIManager.setLookAndFeel("javax.swing.plaf.nimbus.NimbusLookAndFeel") }
        UIManager.put("control", PANEL)
        UIManager.put("info", PANEL_LIGHT)
        UIManager.put("nimbusBase", Color(0x1A, 0x1F, 0x3D))
        UIManager.put("nimbusBlueGrey", Color(0x2A, 0x31, 0x5C))
        UIManager.put("nimbusLightBackground", PANEL_LIGHT)
        UIManager.put("nimbusFocus", ACCENT)
        UIManager.put("nimbusSelectionBackground", Color(0x3B, 0x2A, 0x7A))
        UIManager.put("nimbusSelectedText", TEXT)
        UIManager.put("text", TEXT)
        UIManager.put("defaultFont", Font(Font.SANS_SERIF, Font.PLAIN, 13))
    }

    fun section(title: String): Border = BorderFactory.createCompoundBorder(
        BorderFactory.createEmptyBorder(6, 6, 6, 6),
        BorderFactory.createTitledBorder(BorderFactory.createLineBorder(PANEL_LIGHT), title, 0, 0, null, ACCENT),
    )

    fun button(text: String, tip: String? = null, action: () -> Unit) = JButton(text).apply {
        isFocusable = false
        toolTipText = tip
        addActionListener { action() }
    }

    fun parse(hex: String): Color {
        val h = hex.removePrefix("#")
        return when (h.length) {
            6 -> Color(h.toInt(16))
            8 -> Color(h.substring(0, 2).toInt(16), h.substring(2, 4).toInt(16), h.substring(4, 6).toInt(16), h.substring(6, 8).toInt(16))
            else -> Color.MAGENTA
        }
    }

    fun hex(c: Color): String = if (c.alpha == 255) String.format("#%02X%02X%02X", c.red, c.green, c.blue)
    else String.format("#%02X%02X%02X%02X", c.red, c.green, c.blue, c.alpha)
}
