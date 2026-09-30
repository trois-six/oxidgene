//! Printing through the platform's own print dialog.
//!
//! A WebView's `window.print()` cannot be relied on: WKWebView on macOS
//! ignores it, and whether WebKitGTK answers it depends on the embedder. The
//! WebView itself can always print, through wry — a WebKitGTK print operation
//! on Linux, an `NSPrintOperation` on macOS, WebView2's own dialog on
//! Windows — and every one of them lays the page out with its print media
//! rules, which is all the UI needs.

use std::sync::Arc;

use oxidgene_ui::components::print::{PagePrinter, PrintBridge};

/// Opens the native print dialog on the application window.
struct NativePrinter;

impl PagePrinter for NativePrinter {
    fn print(&self) {
        // Called from the print button's handler, inside the window's own
        // Dioxus runtime, which is what `window()` reads.
        dioxus::desktop::window().print();
    }
}

/// The bridge the print action looks for.
pub fn bridge() -> PrintBridge {
    PrintBridge::new(Arc::new(NativePrinter))
}
