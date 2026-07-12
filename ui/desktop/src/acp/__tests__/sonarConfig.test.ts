import { describe, expect, it } from 'vitest';
import { SONAR_DEFAULT_RELAYS, splitSonarValues } from '../sonarConfig';

describe('Sonar configuration', () => {
  it('uses the five common Sonar relays in priority order', () => {
    expect(SONAR_DEFAULT_RELAYS).toEqual([
      'wss://nostr.relay.hedwig.sh/',
      'wss://relay.damus.io',
      'wss://nos.lol',
      'wss://relay.primal.net',
      'wss://relay.kaleidoswap.com',
    ]);
  });

  it('parses editable comma and newline separated values', () => {
    expect(splitSonarValues(' npub1first,\nnpub1second ,, ')).toEqual([
      'npub1first',
      'npub1second',
    ]);
  });
});
