export const SONAR_DEFAULT_RELAYS = [
  'wss://nostr.relay.hedwig.sh/',
  'wss://relay.damus.io',
  'wss://nos.lol',
  'wss://relay.primal.net',
  'wss://relay.kaleidoswap.com',
] as const;

export function splitSonarValues(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}
