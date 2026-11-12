package com.pixel.studio.desktop

import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Graphics
import java.awt.GridLayout
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.DefaultListModel
import javax.swing.JColorChooser
import javax.swing.JLabel
import javax.swing.JList
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.ListCellRenderer
import javax.swing.ListSelectionModel

/** Current color + palette swatches. Click = pick, double-click = edit entry (recolors art). */
class PalettePanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val grid = JPanel(GridLayout(0, 8, 2, 2))
    private val current = object : JPanel() {
        override fun paintComponent(g: Graphics) {
            super.paintComponent(g)
            g.color = Theme.parse(c.color)
            g.fillRect(2, 2, width - 4, height - 4)
        }
    }

    init {
        border = Theme.section("Palette")
        background = Theme.PANEL
        grid.background = Theme.PANEL
        current.preferredSize = Dimension(40, 28)
        current.toolTipText = "Current color — click to choose any color"
        current.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                JColorChooser.showDialog(this@PalettePanel, "Color", Theme.parse(c.color))?.let {
                    c.color = Theme.hex(it)
                    c.run("palette_add", mapOf("colors" to listOf(c.color)))
                    rebuild()
                }
            }
        })
        val top = JPanel(BorderLayout(6, 0)).apply {
            background = Theme.PANEL
            add(current, BorderLayout.WEST)
            add(JLabel("Click a swatch • double-click edits").apply { foreground = Theme.TEXT_DIM }, BorderLayout.CENTER)
        }
        add(top, BorderLayout.NORTH)
        add(grid, BorderLayout.CENTER)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun rebuild() {
        grid.removeAll()
        c.doc.palette.forEachIndexed { i, hex ->
            val sw = object : JPanel() {
                override fun paintComponent(g: Graphics) {
                    g.color = Theme.parse(hex)
                    g.fillRect(0, 0, width, height)
                    if (hex.equals(c.color, ignoreCase = true)) {
                        g.color = Color.WHITE
                        g.drawRect(0, 0, width - 1, height - 1)
                    }
                }
            }
            sw.preferredSize = Dimension(20, 20)
            sw.toolTipText = "#${i + 1} $hex"
            sw.addMouseListener(object : MouseAdapter() {
                override fun mouseClicked(e: MouseEvent) {
                    if (e.clickCount >= 2) {
                        JColorChooser.showDialog(this@PalettePanel, "Edit palette entry ${i + 1}", Theme.parse(hex))?.let {
                            c.run("palette_set", mapOf("index" to i + 1, "color" to Theme.hex(it)))
                        }
                    } else {
                        c.color = hex
                        rebuild()
                    }
                }
            })
            grid.add(sw)
        }
        current.repaint()
        revalidate()
        repaint()
    }
}

/** Layers, top of the stack first. */
class LayersPanel(private val c: EditorController) : JPanel(BorderLayout()) {
    private val model = DefaultListModel<LayerInfo>()
    private val list = JList(model)
    private var updating = false

    init {
        border = Theme.section("Layers")
        background = Theme.PANEL
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.cellRenderer = ListCellRenderer<LayerInfo> { _, value, _, selected, _ ->
            JLabel((if (value.visible) "◉ " else "○ ") + value.name + (if (value.locked) "  [locked]" else "") + (if (value.opacity < 255) "  ${value.opacity * 100 / 255}%" else "") + if (value.blend != "normal") "  · ${value.blend}" else "").apply {
                isOpaque = true
                border = BorderFactory.createEmptyBorder(4, 8, 4, 8)
                background = if (selected) Theme.PANEL_LIGHT else Theme.PANEL
                foreground = if (value.visible) Theme.TEXT else Theme.TEXT_DIM
            } as Component
        }
        list.addListSelectionListener {
            if (!updating && !it.valueIsAdjusting) list.selectedValue?.let { l -> c.run("layer_select", mapOf("layer" to l.index)) }
        }
        list.addMouseListener(object : MouseAdapter() {
            override fun mouseClicked(e: MouseEvent) {
                val idx = list.locationToIndex(e.point).takeIf { it >= 0 } ?: return
                val l = model[idx]
                if (e.x < 22) c.run("layer_visibility", mapOf("layer" to l.index, "visible" to !l.visible))
                else if (e.clickCount == 2) rename(l)
            }
        })
        add(JScrollPane(list).apply { preferredSize = Dimension(220, 160) }, BorderLayout.CENTER)
        val buttons = JPanel(FlowLayout(FlowLayout.LEFT, 2, 2)).apply {
            background = Theme.PANEL
            add(Theme.button("+", "New layer") { c.run("layer_create") })
            add(Theme.button("−", "Delete layer") {
                selected()?.let {
                    if (JOptionPane.showConfirmDialog(this, "Delete layer '${it.name}' in every frame?", "Delete layer", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                        c.run("layer_delete", mapOf("layer" to it.index))
                    }
                }
            })
            add(Theme.button("▲", "Move up") { selected()?.let { if (it.index < c.doc.layers.size - 1) c.run("layer_move", mapOf("layer" to it.index, "index" to it.index + 1)) } })
            add(Theme.button("▼", "Move down") { selected()?.let { if (it.index > 0) c.run("layer_move", mapOf("layer" to it.index, "index" to it.index - 1)) } })
            add(Theme.button("Lock", "Lock / unlock") { selected()?.let { c.run("layer_lock", mapOf("layer" to it.index, "locked" to !it.locked)) } })
        }
        add(buttons, BorderLayout.SOUTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun selected(): LayerInfo? = list.selectedValue

    private fun rename(l: LayerInfo) {
        val name = JOptionPane.showInputDialog(this, "Layer name", l.name) ?: return
        if (name.isNotBlank()) c.run("layer_rename", mapOf("layer" to l.index, "name" to name))
    }

    private fun rebuild() {
        updating = true
        model.clear()
        c.doc.layers.reversed().forEach { model.addElement(it) }
        val activeRow = c.doc.layers.size - 1 - c.doc.activeLayer
        if (activeRow in 0 until model.size()) list.selectedIndex = activeRow
        updating = false
    }
}

/** Live list of what connected agents have been doing. */
class ActivityPanel(private val c: EditorController) : JPanel() {
    private val model = DefaultListModel<String>()

    init {
        layout = BoxLayout(this, BoxLayout.Y_AXIS)
        border = Theme.section("Agent activity")
        background = Theme.PANEL
        val list = JList(model).apply { foreground = Theme.TEXT_DIM; font = font.deriveFont(11f) }
        add(JScrollPane(list).apply { preferredSize = Dimension(220, 140) })
        c.onChange { rebuild() }
        rebuild()
    }

    private fun rebuild() {
        model.clear()
        c.recentAgentActivity().forEach { model.addElement(it) }
        if (model.isEmpty) model.addElement("No agent edits yet")
    }
}
