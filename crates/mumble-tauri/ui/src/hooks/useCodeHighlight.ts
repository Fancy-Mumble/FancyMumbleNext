import { useEffect } from "react";
import hljs from "highlight.js/lib/common";
import "highlight.js/styles/github-dark.css";

const HIGHLIGHTED = "data-hljs-highlighted";

function highlightAllPending(root: ParentNode): void {
  const blocks = root.querySelectorAll<HTMLElement>(
    `pre > code:not([${HIGHLIGHTED}])`,
  );
  blocks.forEach((block) => {
    try {
      hljs.highlightElement(block);
    } catch {
      // hljs throws on already-highlighted elements; ignore.
    }
    block.setAttribute(HIGHLIGHTED, "true");
  });
}

/**
 * Globally watches the document for `<pre><code>` blocks and applies
 * highlight.js syntax highlighting on a single pass per element.
 *
 * Mounted once at the app root so chat messages, channel descriptions,
 * server welcome text and bios all benefit without each renderer
 * needing to wire its own highlight pass.
 */
export function useCodeHighlight(): void {
  useEffect(() => {
    highlightAllPending(document);

    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        for (const node of Array.from(mutation.addedNodes)) {
          if (node.nodeType === Node.ELEMENT_NODE) {
            highlightAllPending(node as Element);
          }
        }
      }
    });

    observer.observe(document.body, { childList: true, subtree: true });
    return () => observer.disconnect();
  }, []);
}
