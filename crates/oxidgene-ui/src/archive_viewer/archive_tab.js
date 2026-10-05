// Opens an archive's page in a new tab, neither able to reach this page
// (`noopener`) nor told where the reader came from (`noreferrer`).
//
// A browser lets a page open a tab only during the reader's click, and the
// address arrives later, from a lookup on the backend that may query the
// archive's portal for seconds. So the tab is opened blank at once, cut from
// this page, and sent to the address when it arrives, by a link of its own
// document, so that the portal receives no referrer. Should the browser
// refuse even the blank tab, the address is opened in a tab of its own once
// known, which a strict popup blocker may refuse too.
//
// `searching` is defined before this script runs.
const tab = window.open("", "_blank");
if (tab) {
    tab.opener = null;
    const page = tab.document;
    page.title = searching;
    const notice = page.createElement("p");
    notice.textContent = searching;
    (page.body || page.documentElement).appendChild(notice);
}
const url = await dioxus.recv();
const valid = typeof url === "string" && /^https?:\/\//i.test(url);
if (!tab) {
    if (valid) {
        window.open(url, "_blank", "noopener,noreferrer");
    }
} else if (!tab.closed) {
    if (valid) {
        const page = tab.document;
        const link = page.createElement("a");
        link.href = url;
        link.rel = "noopener noreferrer";
        (page.body || page.documentElement).appendChild(link);
        link.click();
    } else {
        tab.close();
    }
}
