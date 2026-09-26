//
//  QueryView.swift
//  Paloma
//
//

import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct QueryView: View {
    let query: QueryModel
    let mode: OverlayMode
    let onSearch: (String) -> Void
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    /// indicator to tell if current input through IME
    @State private var composing = false

    private var placeholder: String {
        switch mode {
        case .search: "Search, or ask anything…"
        case .chat: "Reply…"
        case .session: "Search sessions…"
        }
    }

    private var icon: String {
        switch mode {
        case .search: "magnifyingglass"
        case .chat: "sparkles"
        case .session: "clock.arrow.circlepath"
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20, weight: .light))
                .foregroundStyle(.secondary)
                .padding(.top, 2)
            ZStack(alignment: .topLeading) {
                if query.isEmpty, !composing {
                    Text(placeholder)
                        .font(.system(size: 22, weight: .light))
                        .foregroundStyle(Color(nsColor: .placeholderTextColor))
                }
                ComposerView(
                    query: query,
                    composing: $composing,
                    onSubmit: onSubmit,
                    onNavigate: onNavigate,
                    onEscape: onEscape
                )
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 16)
        .task(id: query.text) {
            if !query.text.isEmpty {
                guard await (try? Task.sleep(for: .milliseconds(150))) != nil else { return }
            }
            onSearch(query.text)
        }
    }
}

private struct ComposerView: NSViewRepresentable {
    let query: QueryModel
    @Binding var composing: Bool
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = ComposerTextView.make()
        textView.delegate = context.coordinator

        let scrollView = NSScrollView()
        scrollView.documentView = textView
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay
        scrollView.hasHorizontalScroller = false

        context.coordinator.attach(textView)
        return scrollView
    }

    func updateNSView(_: NSScrollView, context: Context) {
        context.coordinator.parent = self
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView scrollView: NSScrollView, context _: Context) -> CGSize? {
        guard let textView = scrollView.documentView as? ComposerTextView else { return nil }
        let width = proposal.width ?? textView.frame.width
        if textView.frame.width != width {
            textView.frame.size.width = width
        }
        let cap = ComposerTextView.lineHeight * CGFloat(ComposerTextView.maxLines)
        return CGSize(width: width, height: ceil(min(textView.contentHeight, cap)))
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerView
        private weak var textView: ComposerTextView?
        private var observers: [NSObjectProtocol] = []

        init(_ parent: ComposerView) {
            self.parent = parent
        }

        deinit {
            observers.forEach(NotificationCenter.default.removeObserver)
        }

        func attach(_ textView: ComposerTextView) {
            self.textView = textView
            parent.query.textView = textView
            textView.onSubmit = { [weak self] in self?.parent.onSubmit() }
            textView.onNavigate = { [weak self] delta in self?.parent.onNavigate(delta) }
            textView.onEscape = { [weak self] in self?.parent.onEscape() }
            textView.onMarkedTextChange = { [weak self] in self?.syncComposing() }

            // auto focus on the text view when launcher view is focused
            observers.append(NotificationCenter.default.addObserver(
                forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main
            ) { [weak self] note in
                MainActor.assumeIsolated {
                    guard note.object is PalomaPanel else { return }
                    guard let textView = self?.textView else { return }
                    textView.window?.makeFirstResponder(textView)
                }
            })
            DispatchQueue.main.async {
                textView.window?.makeFirstResponder(textView)
            }
        }

        func textDidChange(_: Notification) {
            parent.query.sync()
            invalidateSize()
        }

        private func syncComposing() {
            let composing = textView?.hasMarkedText() ?? false
            if parent.composing != composing {
                parent.composing = composing
            }
        }

        func invalidateSize() {
            textView?.enclosingScrollView?.invalidateIntrinsicContentSize()
        }
    }
}

final class ComposerTextView: NSTextView {
    private static let font = NSFont.systemFont(ofSize: 22, weight: .light)
    static let lineHeight = NSLayoutManager().defaultLineHeight(for: font)
    static let maxLines = 6
    static let maxImageBytes = 20 * 1024 * 1024

    private static let imageTypes: [UTType] = [.png, .jpeg, .gif, .webP]
    private static let imagePasteboardTypes: [NSPasteboard.PasteboardType] = imageTypes.map { NSPasteboard.PasteboardType($0.identifier) } + [.tiff]
    private static let previewMaxSize = CGSize(width: 360, height: 240)
    private static let previewDelay: TimeInterval = 0.2

    let preview: NSPopover = {
        let popover = NSPopover()
        popover.behavior = .applicationDefined
        return popover
    }()

    private var hoveredImage: NSTextAttachment?
    private var pendingPreview: DispatchWorkItem?

    var onSubmit: () -> Void = {}
    var onNavigate: (Int) -> Void = { _ in }
    var onEscape: () -> Void = {}
    var onMarkedTextChange: () -> Void = {}

    static func make() -> ComposerTextView {
        let textView = ComposerTextView(usingTextLayoutManager: true)
        textView.isRichText = false
        textView.importsGraphics = false
        textView.allowsUndo = true
        textView.usesFontPanel = false
        textView.usesFindBar = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextCompletionEnabled = false
        textView.drawsBackground = false
        textView.font = font
        textView.textColor = .labelColor
        textView.textContainerInset = .zero
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.minSize = NSSize(width: 0, height: lineHeight)
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.setAccessibilityLabel("Query")
        return textView
    }

    var contentHeight: CGFloat {
        guard let layout = textLayoutManager else { return Self.lineHeight }
        layout.ensureLayout(for: layout.documentRange)
        return max(layout.usageBoundsForTextContainer.height, Self.lineHeight)
    }

    func clear() {
        breakUndoCoalescing()
        insertText("", replacementRange: NSRange(location: 0, length: textStorage?.length ?? 0))
    }

    override func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        super.setMarkedText(string, selectedRange: selectedRange, replacementRange: replacementRange)
        onMarkedTextChange()
    }

    override func unmarkText() {
        super.unmarkText()
        onMarkedTextChange()
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        super.insertText(string, replacementRange: replacementRange)
        onMarkedTextChange()
    }

    override func preferredPasteboardType(
        from availableTypes: [NSPasteboard.PasteboardType],
        restrictedToTypesFrom allowedTypes: [NSPasteboard.PasteboardType]?
    ) -> NSPasteboard.PasteboardType? {
        // if it is a file, use fileURL as type so we get the file path
        if availableTypes.contains(.fileURL) {
            return .fileURL
        }
        if !availableTypes.contains(.string), let imageType = Self.imagePasteboardTypes.first(where: availableTypes.contains) {
            return imageType
        }
        return super.preferredPasteboardType(from: availableTypes, restrictedToTypesFrom: allowedTypes)
    }

    /// allow to paste with additional image types
    override var readablePasteboardTypes: [NSPasteboard.PasteboardType] {
        super.readablePasteboardTypes + Self.imagePasteboardTypes
    }

    override func readSelection(from pboard: NSPasteboard, type: NSPasteboard.PasteboardType) -> Bool {
        if Self.imagePasteboardTypes.contains(type), let image = readImageBuffer(from: pboard, type: type) {
            insertText(image, replacementRange: selectedRange())
            return true
        }
        guard type == .fileURL else {
            return super.readSelection(from: pboard, type: type)
        }

        let options: [NSPasteboard.ReadingOptionKey: Any] = [.urlReadingFileURLsOnly: true]
        let fileURLs = pboard.readObjects(forClasses: [NSURL.self], options: options) as? [URL] ?? []
        guard !fileURLs.isEmpty else {
            return super.readSelection(from: pboard, type: type)
        }

        let content = NSMutableAttributedString()
        for fileURL in fileURLs {
            content.append(readImageFile(at: fileURL) ?? NSAttributedString(
                string: Self.singleQuoted(fileURL.path(percentEncoded: false)),
                attributes: typingAttributes
            ))
            content.append(NSAttributedString(string: " ", attributes: typingAttributes))
        }
        insertText(content, replacementRange: selectedRange())
        return true
    }

    /// read image buffer from pasteboard
    private func readImageBuffer(from pboard: NSPasteboard, type: NSPasteboard.PasteboardType) -> NSAttributedString? {
        guard let data = pboard.data(forType: type), data.count <= Self.maxImageBytes else { return nil }
        guard type == .tiff else {
            return image(data, type: UTType(type.rawValue))
        }
        let png = NSBitmapImageRep(data: data)?.representation(using: .png, properties: [:])
        return png.flatMap { image($0, type: .png) }
    }

    /// read image from file
    private func readImageFile(at fileURL: URL) -> NSAttributedString? {
        guard let values = try? fileURL.resourceValues(forKeys: [.contentTypeKey, .fileSizeKey]),
              let contentType = values.contentType,
              let imageType = Self.imageTypes.first(where: contentType.conforms),
              let size = values.fileSize, size <= Self.maxImageBytes,
              let data = try? Data(contentsOf: fileURL)
        else {
            return nil
        }
        return image(data, type: imageType)
    }

    private func image(_ data: Data, type: UTType?) -> NSAttributedString? {
        guard let type, let image = NSImage(data: data), image.size.height > 0 else { return nil }
        guard let thumbnail = ImageThumbnail.image(image, height: Self.lineHeight) else { return nil }
        let attachment = ImageAttachment(data: data, type: type, thumbnail: thumbnail)
        attachment.bounds = CGRect(origin: CGPoint(x: 0, y: Self.font.descender), size: thumbnail.size)
        let content = NSMutableAttributedString(attachment: attachment)
        content.addAttributes(typingAttributes, range: NSRange(location: 0, length: content.length))
        return content
    }

    /// make sure when dragg and drop images, still stay focus
    override func concludeDragOperation(_ sender: NSDraggingInfo?) {
        super.concludeDragOperation(sender)
        window?.makeKeyAndOrderFront(nil)
        window?.makeFirstResponder(self)
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        updatePreview(at: convert(event.locationInWindow, from: nil))
    }

    override func mouseExited(with event: NSEvent) {
        super.mouseExited(with: event)
        hidePreview()
    }

    /// check if pointer is on an image attachment and return associate reference
    func imageAttachment(at point: CGPoint) -> (attachment: NSTextAttachment, frame: CGRect)? {
        let origin = textContainerOrigin
        let location = CGPoint(x: point.x - origin.x, y: point.y - origin.y)
        guard let storage = textStorage, let layout = textLayoutManager, let content = layout.textContentManager,
              let fragment = layout.textLayoutFragment(for: location)
        else {
            return nil
        }
        let fragmentStart = content.offset(from: content.documentRange.location, to: fragment.rangeInElement.location)
        let fragmentLength = content.offset(from: fragment.rangeInElement.location, to: fragment.rangeInElement.endLocation)
        var hit: (attachment: NSTextAttachment, frame: CGRect)?
        storage.enumerateAttribute(.attachment, in: NSRange(location: fragmentStart, length: fragmentLength)) { value, range, stop in
            guard let attachment = value as? NSTextAttachment,
                  let attachmentLocation = content.location(content.documentRange.location, offsetBy: range.location)
            else {
                return
            }
            let frame = fragment.frameForTextAttachment(at: attachmentLocation).offsetBy(
                dx: fragment.layoutFragmentFrame.minX + origin.x,
                dy: fragment.layoutFragmentFrame.minY + origin.y
            )
            if frame.contains(point) {
                hit = (attachment, frame)
                stop.pointee = true
            }
        }
        return hit
    }

    private func updatePreview(at point: CGPoint) {
        guard let (attachment, frame) = imageAttachment(at: point) else {
            hidePreview()
            return
        }
        guard attachment !== hoveredImage else { return }
        hidePreview()
        hoveredImage = attachment
        let work = DispatchWorkItem { [weak self] in
            self?.showPreview(of: attachment, relativeTo: frame)
        }
        pendingPreview = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.previewDelay, execute: work)
    }

    private func showPreview(of attachment: NSTextAttachment, relativeTo frame: CGRect) {
        guard let data = attachment.contents, let image = NSImage(data: data), image.size.width > 0, image.size.height > 0 else {
            return
        }
        let scale = min(1, Self.previewMaxSize.width / image.size.width, Self.previewMaxSize.height / image.size.height)
        let size = CGSize(width: image.size.width * scale, height: image.size.height * scale)
        let imageView = NSImageView(image: image)
        imageView.imageScaling = .scaleProportionallyUpOrDown
        imageView.frame = CGRect(origin: .zero, size: size)
        let controller = NSViewController()
        controller.view = imageView
        preview.contentViewController = controller
        preview.contentSize = size
        preview.show(relativeTo: frame, of: self, preferredEdge: .maxY)
    }

    private func hidePreview() {
        pendingPreview?.cancel()
        pendingPreview = nil
        hoveredImage = nil
        if preview.isShown {
            preview.close()
        }
    }

    override func doCommand(by selector: Selector) {
        switch selector {
        case #selector(insertNewline(_:)):
            // intercept Enter and only keep Shift+Enter as insert new line
            if NSApp.currentEvent?.modifierFlags.contains(.shift) == true {
                super.doCommand(by: selector)
            } else {
                onSubmit()
            }
        case #selector(moveUp(_:)):
            if isOnEdge(.up) {
                onNavigate(-1)
            } else {
                super.doCommand(by: selector)
            }
        case #selector(moveDown(_:)):
            if isOnEdge(.down) {
                onNavigate(1)
            } else {
                super.doCommand(by: selector)
            }
        case #selector(cancelOperation(_:)):
            onEscape()
        case #selector(insertTab(_:)):
            break
        default:
            super.doCommand(by: selector)
        }
    }

    func isOnEdge(_ direction: NSTextSelectionNavigation.Direction) -> Bool {
        guard let layout = textLayoutManager,
              let selection = layout.textSelections.first,
              let destination = layout.textSelectionNavigation.destinationSelection(
                  for: selection,
                  direction: direction,
                  destination: .character,
                  extending: false,
                  confined: false
              ),
              let current = caretY(of: selection, in: layout),
              let target = caretY(of: destination, in: layout)
        else {
            return false
        }
        return abs(target - current) < 0.5
    }

    /// Get the current line for caret
    func caretY(of selection: NSTextSelection, in layout: NSTextLayoutManager) -> CGFloat? {
        guard let location = selection.textRanges.first?.location else { return nil }
        var y: CGFloat?
        layout.enumerateTextSegments(
            in: NSTextRange(location: location),
            type: .selection,
            options: selection.affinity == .upstream ? .upstreamAffinity : []
        ) { _, frame, _, _ in
            y = frame.minY
            return false
        }
        return y
    }

    private static func singleQuoted(_ path: String) -> String {
        "'" + path.replacingOccurrences(of: "'", with: #"'\''"#) + "'"
    }
}

final class ImageAttachment: NSTextAttachment {
    let thumbnail: NSImage

    init(data: Data, type: UTType, thumbnail: NSImage) {
        self.thumbnail = thumbnail
        super.init(data: data, ofType: type.identifier)
    }

    required init?(coder _: NSCoder) {
        nil
    }

    override func image(forBounds _: CGRect, textContainer _: NSTextContainer?, characterIndex _: Int) -> NSImage? {
        thumbnail
    }
}
