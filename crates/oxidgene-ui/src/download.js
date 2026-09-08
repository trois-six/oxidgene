// Only control messages cross the Dioxus bridge, never file contents.
let ready = false;
let writable;
try {
    let handle;
    if (typeof window.showSaveFilePicker === "function") {
        handle = await window.showSaveFilePicker({ suggestedName: fileName });
    }
    dioxus.send("ready");
    ready = true;
    const endpoint = await dioxus.recv();
    if (endpoint === null) return "cancelled";

    // Open the destination before fetching, so write failures do not waste a download.
    if (handle) writable = await handle.createWritable();
    const response = await fetch(endpoint);
    if (!response.ok || !response.body) throw new Error("download failed");
    if (writable) {
        await response.body.pipeTo(writable);
        writable = undefined;
    } else {
        // Firefox and Safari lack the save picker. Keep their fallback native:
        // no WASM buffer, numeric JSON array, or generated JavaScript payload.
        const blob = await response.blob();
        const url = URL.createObjectURL(blob);
        try {
            const anchor = document.createElement("a");
            anchor.href = url;
            anchor.download = fileName;
            document.body.appendChild(anchor);
            try {
                anchor.click();
            } finally {
                anchor.remove();
            }
        } finally {
            setTimeout(() => URL.revokeObjectURL(url), 60000);
        }
    }
    return "saved";
} catch (error) {
    if (writable) {
        try { await writable.abort(); } catch (_) { /* pipeTo may have aborted it. */ }
    }
    const status = !ready && error.name === "AbortError" ? "cancelled" : "failed";
    if (!ready) dioxus.send(status);
    return status;
}
