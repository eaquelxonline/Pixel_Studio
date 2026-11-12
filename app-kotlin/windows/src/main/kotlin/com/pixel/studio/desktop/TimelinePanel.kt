package com.pixel.studio.desktop

import java.awt.BorderLayout
import java.awt.Color
import java.awt.Component
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Graphics
import java.awt.Graphics2D
import java.awt.RenderingHints
import java.awt.image.BufferedImage
import javax.swing.BorderFactory
import javax.swing.DefaultListModel
import javax.swing.JCheckBox
import javax.swing.JLabel
import javax.swing.JList
import javax.swing.JOptionPane
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JSpinner
import javax.swing.ListCellRenderer
import javax.swing.ListSelectionModel
import javax.swing.SpinnerNumberModel
import javax.swing.Timer

/** Frame strip with thumbnails, per-frame durations, onion skin and playback. */
class TimelinePanel(private val c: EditorController, private val canvas: CanvasView) : JPanel(BorderLayout()) {
    private val model = DefaultListModel<FrameInfo>()
    private val list = JList(model)
    private val thumbs = HashMap<Int, BufferedImage>()
    private var thumbRevision = -1L
    private var updating = false
    private val duration = JSpinner(SpinnerNumberModel(100, 1, 60000, 10))
    private val info = JLabel()
    private var timer: Timer? = null
    private var playIndex = 0

    init {
        border = Theme.section("Timeline")
        background = Theme.PANEL
        list.layoutOrientation = JList.HORIZONTAL_WRAP
        list.visibleRowCount = 1
        list.selectionMode = ListSelectionModel.SINGLE_SELECTION
        list.fixedCellWidth = 76
        list.fixedCellHeight = 84
        list.background = Theme.PANEL
        list.cellRenderer = ListCellRenderer<FrameInfo> { _, f, _, selected, _ -> Cell(f, selected) as Component }
        list.addListSelectionListener {
            if (!updating && !it.valueIsAdjusting) list.selectedValue?.let { f -> c.run("frame_select", mapOf("frame" to f.index)) }
        }
        add(JScrollPane(list).apply { preferredSize = Dimension(600, 108); border = BorderFactory.createEmptyBorder() }, BorderLayout.CENTER)

        val controls = JPanel(FlowLayout(FlowLayout.LEFT, 4, 2)).apply {
            background = Theme.PANEL
            add(Theme.button("▶", "Play / stop") { togglePlay() })
            add(Theme.button("+", "Add empty frame after the current one") { c.run("frame_add", mapOf("index" to c.doc.activeFrame + 1)) })
            add(Theme.button("⧉", "Duplicate frame") { c.run("frame_duplicate", mapOf("frame" to c.doc.activeFrame)) })
            add(Theme.button("−", "Delete frame") {
                if (c.doc.frames.size > 1 && JOptionPane.showConfirmDialog(this, "Delete frame ${c.doc.activeFrame}?", "Delete frame", JOptionPane.YES_NO_OPTION) == JOptionPane.YES_OPTION) {
                    c.run("frame_delete", mapOf("frame" to c.doc.activeFrame))
                }
            })
            add(Theme.button("◀", "Move frame left") { if (c.doc.activeFrame > 0) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame - 1)) })
            add(Theme.button("▶|", "Move frame right") { if (c.doc.activeFrame < c.doc.frames.size - 1) c.run("frame_move", mapOf("frame" to c.doc.activeFrame, "index" to c.doc.activeFrame + 1)) })
            add(JLabel("  ms").apply { foreground = Theme.TEXT_DIM })
            add(duration)
            add(Theme.button("Set", "Apply duration to the current frame") { c.run("frame_set_duration", mapOf("frame" to c.doc.activeFrame, "duration" to duration.value)) })
            add(JCheckBox("Onion skin").apply {
                isOpaque = false
                foreground = Theme.TEXT
                addActionListener { c.onionSkin = isSelected; canvas.refresh() }
            })
            add(info.apply { foreground = Theme.TEXT_DIM })
        }
        add(controls, BorderLayout.NORTH)
        c.onChange { rebuild() }
        rebuild()
    }

    private fun thumb(index: Int): BufferedImage {
        if (thumbRevision != c.doc.revision) { thumbs.clear(); thumbRevision = c.doc.revision }
        return thumbs.getOrPut(index) { c.renderFrame(index) }
    }

    private inner class Cell(val f: FrameInfo, val selected: Boolean) : JPanel() {
        init { preferredSize = Dimension(76, 84); isOpaque = true }

        override fun paintComponent(g0: Graphics) {
            val g = g0 as Graphics2D
            g.color = if (selected) Theme.PANEL_LIGHT else Theme.PANEL
            g.fillRect(0, 0, width, height)
            val img = thumb(f.index)
            val box = 60
            val s = minOf(box.toDouble() / img.width, box.toDouble() / img.height)
            val w = (img.width * s).toInt(); val h = (img.height * s).toInt()
            val x = (width - w) / 2; val y = 4 + (box - h) / 2
            g.color = Theme.CHECKER_B
            g.fillRect(x, y, w, h)
            g.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR)
            g.drawImage(img, x, y, w, h, null)
            g.color = if (selected) Theme.ACCENT else Color(255, 255, 255, 40)
            g.drawRect(x - 1, y - 1, w + 1, h + 1)
            g.color = Theme.TEXT_DIM
            g.drawString("${f.index}  ${f.duration}ms", 6, height - 6)
        }
    }

    private fun togglePlay() {
        timer?.let { it.stop(); timer = null; return }
        playIndex = c.doc.activeFrame
        val t = Timer(c.doc.frames.getOrNull(playIndex)?.duration ?: 100) {}
        t.addActionListener {
            val frames = c.doc.frames
            if (frames.isEmpty()) return@addActionListener
            playIndex = (playIndex + 1) % frames.size
            c.run("frame_select", mapOf("frame" to playIndex))
            t.delay = frames[playIndex].duration
        }
        t.isRepeats = true
        timer = t
        t.start()
    }

    private fun rebuild() {
        updating = true
        if (model.size() != c.doc.frames.size || (0 until model.size()).any { model[it] != c.doc.frames[it] }) {
            model.clear()
            c.doc.frames.forEach { model.addElement(it) }
        }
        list.selectedIndex = c.doc.activeFrame
        list.ensureIndexIsVisible(c.doc.activeFrame)
        list.repaint()
        c.doc.frames.getOrNull(c.doc.activeFrame)?.let { duration.value = it.duration }
        info.text = "  ${c.doc.frames.size} frames • ${c.doc.totalMs} ms • ${"%.1f".format(c.doc.fps)} fps"
        updating = false
    }
}
