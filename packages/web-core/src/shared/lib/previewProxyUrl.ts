/** Public server previews use a separate HTTPS wildcard origin, never /api on
 * the application's origin. Local desktop previews retain their old URL. */
export function previewProxyUrl(
  source: URL,
  targetPort: string,
  proxyPort: number,
  publicDomain?: string | null,
  relayHostId?: string | null
): URL | null {
  const port = Number(targetPort);
  if (!/^\d+$/.test(targetPort) || port < 1 || port > 65535) return null;
  if (publicDomain) {
    if (
      !/^(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z][a-z0-9-]*$/.test(
        publicDomain
      ) ||
      port < 1024 ||
      [3000, 3001, 8443].includes(port) ||
      relayHostId
    )
      return null;
  }
  const token = relayHostId ? `${port}--${relayHostId}` : String(port);
  const url = new URL(
    publicDomain
      ? `https://${token}.${publicDomain}`
      : `http://${token}.localhost:${proxyPort}`
  );
  // Assign individually so a path beginning with // cannot change the origin.
  url.pathname = source.pathname;
  url.search = source.search;
  url.hash = source.hash;
  return url;
}
