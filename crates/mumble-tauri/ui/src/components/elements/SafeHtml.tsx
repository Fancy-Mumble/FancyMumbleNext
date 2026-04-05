/**
 * SafeHtml - renders untrusted HTML safely.
 *
 * Combines DOMPurify-based sanitization with ExternalLinkGuard so
 * that every piece of user-generated HTML goes through a single,
 * auditable security pipeline.
 *
 * Usage:
 *   <SafeHtml html={channel.description} className={styles.desc} />
 *   <SafeHtml html={welcomeText} fallback={<em>No content</em>} />
 */

import { useMemo } from "react";
import { sanitizeHtml } from "../../utils/sanitizeHtml";
import { ExternalLinkGuard } from "./ExternalLinkGuard";

interface SafeHtmlProps {
  /** Raw (untrusted) HTML string to sanitize and render. */
  readonly html: string;
  /** Optional CSS class applied to the wrapper div. */
  readonly className?: string;
  /** Rendered when `html` is empty or only whitespace. */
  readonly fallback?: React.ReactNode;
}

export function SafeHtml({ html, className, fallback }: SafeHtmlProps) {
  const clean = useMemo(() => sanitizeHtml(html), [html]);

  if (!clean && fallback) {
    return <div className={className}>{fallback}</div>;
  }

  if (!clean) return null;

  return (
    <ExternalLinkGuard className={className}>
      <div dangerouslySetInnerHTML={{ __html: clean }} />
    </ExternalLinkGuard>
  );
}
