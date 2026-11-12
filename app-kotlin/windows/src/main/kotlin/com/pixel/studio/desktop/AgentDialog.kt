package com.pixel.studio.desktop

import com.pixel.studio.gateway.AgentGateway
import java.awt.BorderLayout
import java.awt.Dimension
import java.awt.FlowLayout
import java.awt.Font
import java.awt.Toolkit
import java.awt.datatransfer.StringSelection
import javax.swing.BorderFactory
import javax.swing.BoxLayout
import javax.swing.JDialog
import javax.swing.JFrame
import javax.swing.JLabel
import javax.swing.JPanel
import javax.swing.JScrollPane
import javax.swing.JTextArea

/** "Connect an agent": endpoint, token and copy-paste snippets for Claude Code, Antigravity, etc. */
class AgentDialog(owner: JFrame, private val gateway: AgentGateway, private val onToggle: () -> Unit) : JDialog(owner, "Connect an AI agent", false) {
    private val body = JPanel()

    init {
        body.layout = BoxLayout(body, BoxLayout.Y_AXIS)
        body.background = Theme.PANEL
        body.border = BorderFactory.createEmptyBorder(12, 12, 12, 12)
        contentPane.add(JScrollPane(body), BorderLayout.CENTER)
        rebuild()
        size = Dimension(760, 560)
        setLocationRelativeTo(owner)
    }

    private fun rebuild() {
        body.removeAll()
        val state = if (gateway.running) "Agent Mode is ON — ${gateway.mcpUrl}" else "Agent Mode is OFF"
        body.add(JLabel(state).apply { foreground = if (gateway.running) Theme.ACCENT else Theme.TEXT_DIM; font = font.deriveFont(Font.BOLD, 15f) })
        body.add(JLabel("Agents can only use Pixel Studio's drawing tools. Deleting frames/layers asks you first.").apply { foreground = Theme.TEXT_DIM })
        body.add(JPanel(FlowLayout(FlowLayout.LEFT)).apply {
            isOpaque = false
            add(Theme.button(if (gateway.running) "Stop Agent Mode" else "Start Agent Mode") { onToggle(); rebuild() })
            add(Theme.button("New token", "Disconnects every agent using the old token") { gateway.access.rotateToken(); rebuild() })
        })
        if (gateway.running) {
            for ((title, snippet) in gateway.connectionSnippets()) {
                body.add(JLabel(title).apply { foreground = Theme.ACCENT_2; border = BorderFactory.createEmptyBorder(10, 0, 4, 0) })
                val area = JTextArea(snippet).apply {
                    isEditable = false
                    lineWrap = true
                    font = Font(Font.MONOSPACED, Font.PLAIN, 12)
                    background = Theme.BG
                    foreground = Theme.TEXT
                    border = BorderFactory.createEmptyBorder(6, 6, 6, 6)
                }
                body.add(area)
                body.add(JPanel(FlowLayout(FlowLayout.LEFT, 0, 2)).apply {
                    isOpaque = false
                    add(Theme.button("Copy") { Toolkit.getDefaultToolkit().systemClipboard.setContents(StringSelection(snippet), null) })
                })
            }
            body.add(JLabel("Then ask your agent e.g.: \"Use Pixel Studio. Draw a 32x32 knight, then make a 6-frame idle animation.\"").apply {
                foreground = Theme.TEXT_DIM
                border = BorderFactory.createEmptyBorder(12, 0, 0, 0)
            })
        }
        body.revalidate()
        body.repaint()
    }
}
