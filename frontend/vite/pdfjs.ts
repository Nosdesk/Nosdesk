/**
 * Build support for pdf.js: its runtime assets, and its viewer stylesheet.
 */
import { cpSync, createReadStream, existsSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, dirname, join, normalize } from "node:path";
import postcss, { type Container, type Root } from "postcss";
import type { Plugin } from "vite";

const pdfjsRoot = dirname(createRequire(import.meta.url).resolve("pdfjs-dist/package.json"));

/**
 * pdf.js loads its image decoders (wasm), CMaps, standard fonts, ICC
 * profiles and annotation icons at runtime from the URLs `src/utils/pdfjs.ts`
 * passes it. This
 * copies them from the installed pdfjs-dist into `/pdfjs/<version>/`, read
 * from the same package.json the runtime's `version` comes from, so a new
 * pdf.js never meets a previous release's decoders through the immutable
 * cache the backend sets on `/pdfjs/`. `vite dev` serves them straight from
 * node_modules. quickjs (PDF scripting) is left out: the viewer never runs
 * a document's JavaScript.
 */
export function pdfjsAssets(): Plugin {
  const { version } = JSON.parse(readFileSync(join(pdfjsRoot, "package.json"), "utf8")) as { version: string };
  // Folder in the published `/pdfjs/<version>/` → folder in pdfjs-dist.
  const folders: Record<string, string> = {
    wasm: "wasm",
    cmaps: "cmaps",
    standard_fonts: "standard_fonts",
    iccs: "iccs",
    images: "web/images",
  };
  const prefix = `/pdfjs/${version}/`;
  // From `web/images`, only the icons a text annotation shows (the rest are
  // pdf.js's own toolbar, which the app doesn't use); elsewhere everything
  // but quickjs.
  const copied = (folder: string, name: string) =>
    folder === "images" ? /^annotation-[\w-]+\.svg$/.test(name) : !name.startsWith("quickjs");
  let outDir = "dist";

  return {
    name: "nosdesk-pdfjs-assets",
    configResolved(config) {
      outDir = join(config.root, config.build.outDir);
    },
    // Runs after every build, including each `vite build --watch` rebuild;
    // the copy is skipped once the version's folder is there.
    closeBundle() {
      const target = join(outDir, "pdfjs", version);
      if (existsSync(target)) return;
      for (const [published, source] of Object.entries(folders)) {
        const from = join(pdfjsRoot, source);
        cpSync(from, join(target, published), {
          recursive: true,
          filter: (file) => file === from || copied(published, basename(file)),
        });
      }
    },
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const path = req.url?.split("?")[0] ?? "";
        if (!path.startsWith(prefix)) return next();
        const relative = normalize(path.slice(prefix.length));
        const [folder, ...rest] = relative.split(/[\\/]/);
        const source = Object.hasOwn(folders, folder) ? folders[folder] : undefined;
        const file = source ? join(pdfjsRoot, source, ...rest) : "";
        if (!source || !copied(folder, basename(file)) || !existsSync(file) || !statSync(file).isFile()) {
          return next();
        }
        if (file.endsWith(".wasm")) res.setHeader("Content-Type", "application/wasm");
        if (file.endsWith(".svg")) res.setHeader("Content-Type", "image/svg+xml");
        createReadStream(file).pipe(res);
      });
    },
  };
}

/** The class on the element that wraps the pdf.js viewer. */
export const PDFJS_SCOPE = "pdfjs-scope";

/**
 * pdf.js's viewer stylesheet is written for its own page: it sets variables
 * on `:root` and styles generic classes (`.page`, `.dialog`, `.sidebar`).
 * Loaded as is, it would reach the rest of the app once a PDF had been
 * opened. This confines it to the `.pdfjs-scope` wrapper.
 */
export function scopePdfjsCss(): Plugin {
  return {
    name: "nosdesk-pdfjs-css-scope",
    enforce: "pre",
    transform(code, id) {
      if (!id.split("?")[0].endsWith("pdfjs-dist/web/pdf_viewer.css")) return null;
      return { code: scopeCss(code, `.${PDFJS_SCOPE}`), map: null };
    },
  };
}

/**
 * Prefix every top-level selector with `scope`. `:root`, `html` and `body`
 * become the scope itself. Rules inside `@media`, `@supports` and similar
 * are prefixed too; nested rules and keyframe steps are left alone, since
 * they already sit under a prefixed rule or aren't selectors.
 */
export function scopeCss(css: string, scope: string): string {
  const root = postcss.parse(css);
  scopeContainer(root, scope);
  return root.toString();
}

function scopeContainer(container: Root | Container, scope: string) {
  container.each((node) => {
    if (node.type === "rule") {
      node.selectors = node.selectors.map((selector) => scopeSelector(selector, scope));
    } else if (node.type === "atrule" && node.nodes && !/keyframes$/i.test(node.name) && node.name !== "font-face") {
      scopeContainer(node, scope);
    }
  });
}

function scopeSelector(selector: string, scope: string): string {
  const trimmed = selector.trim();
  const own = /^(:root|html|body)(?![\w-])/.exec(trimmed);
  if (own) return `${scope}${trimmed.slice(own[0].length)}`;
  return `${scope} ${trimmed}`;
}
