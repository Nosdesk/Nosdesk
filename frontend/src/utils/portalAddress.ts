/**
 * The address of an organisation's support portal from what someone typed:
 * its web address (`support.acme.com`, with or without `https://`), or, on
 * Nosdesk Cloud, the short name in front of `portalDomain` (`acme`). `null`
 * when it can't be an address.
 */
export function portalAddressFrom(input: string, portalDomain: string): string | null {
  const host = input
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .split(/[/?#]/, 1)[0]!
    .replace(/\.$/, '')
  if (!/^[a-z0-9](?:[a-z0-9.-]{0,251}[a-z0-9])?$/.test(host) || host.includes('..')) return null
  if (host.includes('.')) return `https://${host}/`
  return portalDomain ? `https://${host}.${portalDomain}/` : null
}
