/** Seeded into userData/custom on first start and never touched again.
 *  `{{FILE}}` is replaced with the path the file was written to. */
export const EXAMPLE_PAGE = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>LIVI custom page</title>
    <!-- LIVI's palette, following the head unit. Drop the link to use your own. -->
    <link rel="stylesheet" href="livi-theme.css" />
    <style>
      body {
        margin: 0;
        min-height: 100vh;
        box-sizing: border-box;
        padding: 1rem;
        display: flex;
        flex-direction: column;
        gap: 0.6rem;
        align-items: center;
        justify-content: center;
        font: clamp(12px, 3vh, 16px)/1.4 system-ui, sans-serif;
        background: var(--livi-background);
        color: var(--livi-text);
      }
      h1 { color: var(--livi-primary); margin: 0; font-size: 1.4em; }
      p { margin: 0; max-width: 36rem; text-align: center; overflow-wrap: anywhere; }
      code {
        padding: 0.2rem 0.45rem;
        border-radius: 6px;
        border: 1px solid var(--livi-divider);
        color: var(--livi-text-secondary);
      }
      #where { color: var(--livi-text-secondary); font-size: 0.9rem; }
    </style>
  </head>
  <body>
    <h1>Custom tab</h1>
    <p><code>{{FILE}}</code></p>
    <p>Edit it, or set an address in the settings to load something else.</p>
    <p>The <code>icon.svg</code> next to it is the tab icon.</p>
  </body>
</html>
`
