// @refresh reload
import { createHandler, StartServer } from "~/runtime/start-server";
import { BRAND_ICON_LINKS } from "~/lib/brand-icon-links";

export default createHandler(() => (
  <StartServer
    document={({ assets, children, scripts }) => (
      <html lang="en">
        <head>
          <meta charset="utf-8" />
          <meta name="viewport" content="width=device-width, initial-scale=1" />
          {BRAND_ICON_LINKS.map((link) => <link key={link.href} {...link} />)}
          {assets}
        </head>
        <body>
          <div id="app">{children}</div>
          {scripts}
        </body>
      </html>
    )}
  />
));
