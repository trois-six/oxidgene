// The crash screen (crash.rs): what replaces the page once the application
// stopped on a bug. The renderer cannot draw any more, so this is plain DOM,
// run by the web binary's panic hook as the body of a function of `title`,
// `message`, `action` (the three texts, already translated) and `report`
// (the panic, for the console only: it is English and may hold identifiers).
if (!document.getElementById("crash-screen")) {
    console.error(`OxidGene stopped: ${report}`);

    // The theme's colours live in a stylesheet the page is about to lose.
    const theme = getComputedStyle(document.documentElement);
    const token = (name, fallback) => theme.getPropertyValue(name).trim() || fallback;
    const background = token("--bg-deep", "#ffffff");
    const text = token("--text-primary", "#1a1a1a");
    const accent = token("--orange", "#b45309");
    const onAccent = token("--on-accent", "#ffffff");
    const font = token("--font-sans", "sans-serif");

    const element = (name, content, style) => {
        const node = document.createElement(name);
        node.textContent = content;
        node.style.cssText = style;
        return node;
    };
    const screen = element(
        "main",
        "",
        `position: fixed; inset: 0; display: flex; flex-direction: column; align-items: center;
         justify-content: center; gap: 16px; padding: 24px; text-align: center;
         background: ${background}; color: ${text}; font-family: ${font};`,
    );
    screen.id = "crash-screen";
    screen.setAttribute("role", "alert");
    const reload = element(
        "button",
        action,
        `padding: 8px 20px; border: 0; border-radius: 6px; font: inherit; font-weight: 600;
         cursor: pointer; background: ${accent}; color: ${onAccent};`,
    );
    reload.type = "button";
    reload.addEventListener("click", () => location.reload());
    screen.append(
        element("h1", title, "margin: 0; font-size: 1.5rem;"),
        element("p", message, "margin: 0; max-width: 34rem; line-height: 1.6;"),
        reload,
    );

    // Everything the application drew goes, open dialogs included: none of
    // it answers any more.
    document.body.replaceChildren(screen);
    reload.focus();
}
